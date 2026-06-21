use std::error::Error as StdError;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde::Serialize;

use crate::protocol::{
    self, ConfigFetch, ConfigLocation, ConfigWrite, GridTarget, Heartbeat, PacketIdentity,
    PageActive, PageStore, ProtocolError,
};
use crate::runtime_bundle::{
    normalize_text_content, normalized_sha256, OwnedRuntimeSlot, RuntimeAsset, RuntimeBundle,
    RuntimeBundleError, RuntimeLayerSpec,
};
use crate::targeting::ResolvedTarget;
use crate::transport::{SerialTransport, TransportError};

const CONNECTION_STABILIZATION_DELAY: Duration = Duration::from_millis(150);
const CONFIG_WRITE_SETTLE_DELAY: Duration = Duration::from_millis(150);
const PAGE_CHANGE_SETTLE_DELAY: Duration = Duration::from_millis(150);
const HEARTBEAT_BOOTSTRAP_DELAY: Duration = Duration::from_millis(300);
const READ_BACK_TOTAL_WINDOW: Duration = Duration::from_millis(1200);
const READ_BACK_IDLE_WINDOW: Duration = Duration::from_millis(250);
const PAGE_STORE_TOTAL_WINDOW: Duration = Duration::from_millis(8000);
const PAGE_STORE_IDLE_WINDOW: Duration = Duration::from_millis(500);
const READ_BACK_POLL_INTERVAL: Duration = Duration::from_millis(20);

const GRID_CONST_ETX: u8 = 0x03;
const GRID_CONST_EOT: u8 = 0x04;
const GRID_CONST_LF: u8 = 0x0a;
const GRID_CLASS_CONFIG: usize = 0x060;
const GRID_CLASS_PAGESTORE: usize = 0x061;
const GRID_INSTR_ACKNOWLEDGE: usize = 0x0a;
const GRID_INSTR_NACKNOWLEDGE: usize = 0x0b;
const GRID_INSTR_REPORT: usize = 0x0d;

pub type Result<T> = std::result::Result<T, RuntimeError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    Bundle(RuntimeBundleError),
    Protocol(ProtocolError),
    Transport(TransportError),
    HostStorage { message: String },
    UnexpectedResponse { message: String },
    VerificationFailed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSlotRead {
    pub source_target: GridTarget,
    pub content: String,
}

pub trait RuntimeSlotReader {
    fn read_owned_slot(
        &mut self,
        target: ResolvedTarget,
        slot: &OwnedRuntimeSlot,
    ) -> Result<Option<RuntimeSlotRead>>;
}

pub trait RuntimeSlotWriter {
    fn write_owned_slot(&mut self, target: ResolvedTarget, asset: &RuntimeAsset) -> Result<()>;
}

pub trait RuntimePageStorer {
    fn store_page(&mut self, target: ResolvedTarget, page: u8) -> Result<()>;
}

pub trait RuntimeSlotClearer {
    fn clear_owned_slot(&mut self, target: ResolvedTarget, slot: &OwnedRuntimeSlot) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeSlotStatus {
    Match { source_target: GridTarget },
    Missing,
    Drifted { source_target: GridTarget },
    WrongTarget { actual_target: GridTarget },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSlotInspection {
    pub slot: OwnedRuntimeSlot,
    pub status: RuntimeSlotStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInspectionReport {
    bundle_version: String,
    requested_target: ResolvedTarget,
    observed_targets: Vec<GridTarget>,
    slot_inspections: Vec<RuntimeSlotInspection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInstallReport {
    installed_slots: Vec<OwnedRuntimeSlot>,
    verification_report: RuntimeInspectionReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeUpgradeReport {
    install_report: RuntimeInstallReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRemoveReport {
    removed_slots: Vec<OwnedRuntimeSlot>,
    restored_from_backup: bool,
    warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct StoredRuntimeManifest {
    bundle_version: String,
    compatibility_reference: String,
    runtime_marker: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    compatibility_notes: Vec<String>,
    layers: Vec<RuntimeLayerSpec>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    owned_slots: Vec<StoredOwnedRuntimeSlot>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct StoredOwnedRuntimeSlot {
    name: String,
    page: u8,
    element: u8,
    event: u8,
    asset: String,
    install_order: u32,
    runtime_marker: String,
}

pub struct TransportRuntimeSlotReader<T> {
    transport: T,
    session_id: u8,
    next_message_id: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfigFetchReport {
    source_target: GridTarget,
    content: String,
}

impl RuntimeError {
    pub fn verification_failed(message: impl Into<String>) -> Self {
        Self::VerificationFailed {
            message: message.into(),
        }
    }

    pub fn unexpected_response(message: impl Into<String>) -> Self {
        Self::UnexpectedResponse {
            message: message.into(),
        }
    }

    pub fn host_storage(message: impl Into<String>) -> Self {
        Self::HostStorage {
            message: message.into(),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bundle(error) => error.fmt(f),
            Self::Protocol(error) => error.fmt(f),
            Self::Transport(error) => error.fmt(f),
            Self::HostStorage { message } => write!(f, "runtime storage failed: {message}"),
            Self::UnexpectedResponse { message } => {
                write!(f, "runtime inspection failed: {message}")
            }
            Self::VerificationFailed { message } => {
                write!(f, "runtime verification failed: {message}")
            }
        }
    }
}

impl StdError for RuntimeError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Bundle(error) => Some(error),
            Self::Protocol(error) => Some(error),
            Self::Transport(error) => Some(error),
            Self::HostStorage { .. }
            | Self::UnexpectedResponse { .. }
            | Self::VerificationFailed { .. } => None,
        }
    }
}

impl From<RuntimeBundleError> for RuntimeError {
    fn from(value: RuntimeBundleError) -> Self {
        Self::Bundle(value)
    }
}

impl From<ProtocolError> for RuntimeError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<TransportError> for RuntimeError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

impl RuntimeInspectionReport {
    pub fn bundle_version(&self) -> &str {
        &self.bundle_version
    }

    pub fn requested_target(&self) -> ResolvedTarget {
        self.requested_target
    }

    pub fn observed_target(&self) -> Option<GridTarget> {
        match self.observed_targets.as_slice() {
            [target] => Some(*target),
            _ => None,
        }
    }

    pub fn observed_targets(&self) -> &[GridTarget] {
        &self.observed_targets
    }

    pub fn slot_inspections(&self) -> &[RuntimeSlotInspection] {
        &self.slot_inspections
    }

    pub fn is_exact_match(&self) -> bool {
        if !self
            .slot_inspections
            .iter()
            .all(|inspection| matches!(inspection.status, RuntimeSlotStatus::Match { .. }))
        {
            return false;
        }

        match self.requested_target {
            ResolvedTarget::Broadcast => self.observed_targets.len() == 1,
            ResolvedTarget::Explicit(expected) => self.observed_targets.as_slice() == [expected],
        }
    }

    pub fn status_label(&self) -> &'static str {
        if self.is_exact_match() {
            "exact-match compatible"
        } else {
            "not exact-match compatible"
        }
    }

    pub fn verification_failure_summary(&self) -> String {
        let mut details = Vec::new();

        if matches!(self.requested_target, ResolvedTarget::Broadcast)
            && self.observed_targets.len() > 1
        {
            details.push(format!(
                "owned slot reports came back from multiple module targets ({})",
                format_targets(&self.observed_targets)
            ));
        }

        for inspection in &self.slot_inspections {
            let detail = match &inspection.status {
                RuntimeSlotStatus::Match { .. } => None,
                RuntimeSlotStatus::Missing => Some(format!(
                    "{} at {} is missing or blank",
                    inspection.slot.name,
                    inspection.slot.location_display()
                )),
                RuntimeSlotStatus::Drifted { source_target } => Some(format!(
                    "{} at {} drifted on dx={} dy={} with content mismatch",
                    inspection.slot.name,
                    inspection.slot.location_display(),
                    source_target.dx,
                    source_target.dy,
                )),
                RuntimeSlotStatus::WrongTarget { actual_target } => Some(format!(
                    "{} at {} responded from dx={} dy={} instead of the requested target",
                    inspection.slot.name,
                    inspection.slot.location_display(),
                    actual_target.dx,
                    actual_target.dy
                )),
            };

            if let Some(detail) = detail {
                details.push(detail);
            }
        }

        if details.is_empty() {
            "the owned slot reports were incomplete or inconsistent".to_string()
        } else {
            details.join("; ")
        }
    }
}

impl RuntimeInstallReport {
    pub fn installed_slots(&self) -> &[OwnedRuntimeSlot] {
        &self.installed_slots
    }

    pub fn verification_report(&self) -> &RuntimeInspectionReport {
        &self.verification_report
    }
}

impl RuntimeUpgradeReport {
    pub fn install_report(&self) -> &RuntimeInstallReport {
        &self.install_report
    }
}

impl RuntimeRemoveReport {
    pub fn removed_slots(&self) -> &[OwnedRuntimeSlot] {
        &self.removed_slots
    }

    pub fn restored_from_backup(&self) -> bool {
        self.restored_from_backup
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }
}

impl<T> TransportRuntimeSlotReader<T>
where
    T: SerialTransport,
{
    pub fn new(mut transport: T) -> Result<Self> {
        thread::sleep(CONNECTION_STABILIZATION_DELAY);
        transport.clear_input()?;

        Ok(Self {
            transport,
            session_id: 1,
            next_message_id: 1,
        })
    }

    fn next_identity(&mut self) -> PacketIdentity {
        let identity = PacketIdentity::new(self.session_id, self.next_message_id);

        self.next_message_id = self.next_message_id.wrapping_add(1);
        if self.next_message_id == 0 {
            self.next_message_id = 1;
            self.session_id = self.session_id.wrapping_add(1);
            if self.session_id == 0 {
                self.session_id = 1;
            }
        }

        identity
    }
}

impl<T> RuntimeSlotReader for TransportRuntimeSlotReader<T>
where
    T: SerialTransport,
{
    fn read_owned_slot(
        &mut self,
        target: ResolvedTarget,
        slot: &OwnedRuntimeSlot,
    ) -> Result<Option<RuntimeSlotRead>> {
        self.transport.clear_input()?;

        let heartbeat_packet = protocol::encode_heartbeat_packet(&Heartbeat {
            identity: self.next_identity(),
        })?;
        self.transport.write_config(&heartbeat_packet)?;
        thread::sleep(HEARTBEAT_BOOTSTRAP_DELAY);

        let fetch_packet = protocol::encode_config_fetch_packet(&ConfigFetch {
            target: target.grid_target(),
            location: ConfigLocation::new(slot.page, slot.element, slot.event),
            identity: self.next_identity(),
        })?;
        self.transport.write_config(&fetch_packet)?;

        let inbound = read_transport_until_idle(
            &mut self.transport,
            READ_BACK_TOTAL_WINDOW,
            READ_BACK_IDLE_WINDOW,
        )?;

        extract_single_config_report(&inbound, slot).map(|report| {
            report.map(|report| RuntimeSlotRead {
                source_target: report.source_target,
                content: report.content,
            })
        })
    }
}

impl<T> RuntimeSlotWriter for TransportRuntimeSlotReader<T>
where
    T: SerialTransport,
{
    fn write_owned_slot(&mut self, target: ResolvedTarget, asset: &RuntimeAsset) -> Result<()> {
        self.transport.clear_input()?;

        let packet = protocol::encode_config_packet(&ConfigWrite {
            target: target.grid_target(),
            location: ConfigLocation::new(asset.slot.page, asset.slot.element, asset.slot.event),
            lua: &asset.normalized_content,
            identity: self.next_identity(),
        })?;
        self.transport.write_config(&packet)?;
        thread::sleep(CONFIG_WRITE_SETTLE_DELAY);

        Ok(())
    }
}

impl<T> RuntimePageStorer for TransportRuntimeSlotReader<T>
where
    T: SerialTransport,
{
    fn store_page(&mut self, target: ResolvedTarget, page: u8) -> Result<()> {
        self.transport.clear_input()?;

        let page_active_packet = protocol::encode_page_active_packet(&PageActive {
            target: target.grid_target(),
            page,
            identity: self.next_identity(),
        })?;
        self.transport.write_config(&page_active_packet)?;
        thread::sleep(PAGE_CHANGE_SETTLE_DELAY);

        let page_store_packet = protocol::encode_page_store_packet(&PageStore {
            target: target.grid_target(),
            identity: self.next_identity(),
        })?;
        self.transport.write_config(&page_store_packet)?;

        let inbound = read_transport_until_idle(
            &mut self.transport,
            PAGE_STORE_TOTAL_WINDOW,
            PAGE_STORE_IDLE_WINDOW,
        )?;

        extract_page_store_ack(&inbound, target)
    }
}

impl<T> RuntimeSlotClearer for TransportRuntimeSlotReader<T>
where
    T: SerialTransport,
{
    fn clear_owned_slot(&mut self, target: ResolvedTarget, slot: &OwnedRuntimeSlot) -> Result<()> {
        self.transport.clear_input()?;

        let packet = protocol::encode_config_packet(&ConfigWrite {
            target: target.grid_target(),
            location: ConfigLocation::new(slot.page, slot.element, slot.event),
            lua: "",
            identity: self.next_identity(),
        })?;
        self.transport.write_config(&packet)?;
        thread::sleep(CONFIG_WRITE_SETTLE_DELAY);

        Ok(())
    }
}

pub fn inspect_bundled_runtime<R>(
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeInspectionReport>
where
    R: RuntimeSlotReader,
{
    inspect_runtime_with_bundle_dir(
        crate::runtime_bundle::bundled_runtime_dir(),
        requested_target,
        reader,
    )
}

pub fn inspect_runtime_with_bundle_dir<R>(
    bundle_dir: impl AsRef<Path>,
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeInspectionReport>
where
    R: RuntimeSlotReader,
{
    let bundle = RuntimeBundle::load_from_dir(bundle_dir)?;
    inspect_runtime_bundle(&bundle, requested_target, reader)
}

pub fn inspect_installed_runtime<R>(
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<Option<RuntimeInspectionReport>>
where
    R: RuntimeSlotReader,
{
    inspect_runtime_with_optional_bundle_dir(
        installed_runtime_dir().as_deref(),
        requested_target,
        reader,
    )
}

pub fn verify_bundled_runtime<R>(
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeInspectionReport>
where
    R: RuntimeSlotReader,
{
    verify_runtime_with_bundle_dir(
        crate::runtime_bundle::bundled_runtime_dir(),
        requested_target,
        reader,
    )
}

pub fn verify_runtime_with_bundle_dir<R>(
    bundle_dir: impl AsRef<Path>,
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeInspectionReport>
where
    R: RuntimeSlotReader,
{
    let report = inspect_runtime_with_bundle_dir(bundle_dir, requested_target, reader)?;

    if report.is_exact_match() {
        Ok(report)
    } else {
        Err(RuntimeError::verification_failed(format!(
            "bundled runtime {} is not an exact match: {}",
            report.bundle_version(),
            report.verification_failure_summary()
        )))
    }
}

pub fn verify_installed_runtime<R>(
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeInspectionReport>
where
    R: RuntimeSlotReader,
{
    verify_runtime_with_optional_bundle_dir(
        installed_runtime_dir().as_deref(),
        requested_target,
        reader,
    )
}

fn inspect_runtime_with_optional_bundle_dir<R>(
    bundle_dir: Option<&Path>,
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<Option<RuntimeInspectionReport>>
where
    R: RuntimeSlotReader,
{
    let Some(bundle_dir) = bundle_dir.filter(|path| path.is_dir()) else {
        return Ok(None);
    };

    let report = inspect_runtime_with_bundle_dir(bundle_dir, requested_target, reader)?;
    Ok(Some(report))
}

fn verify_runtime_with_optional_bundle_dir<R>(
    bundle_dir: Option<&Path>,
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeInspectionReport>
where
    R: RuntimeSlotReader,
{
    let Some(report) =
        inspect_runtime_with_optional_bundle_dir(bundle_dir, requested_target, reader)?
    else {
        return Err(RuntimeError::verification_failed(
            "no frozen installed runtime was found under ~/.config/vsn1-cli/runtime",
        ));
    };

    if report.is_exact_match() {
        Ok(report)
    } else {
        Err(RuntimeError::verification_failed(format!(
            "installed runtime {} is not an exact match: {}",
            report.bundle_version(),
            report.verification_failure_summary()
        )))
    }
}

fn install_runtime_bundle_with_storage<R>(
    bundle: &RuntimeBundle,
    requested_target: ResolvedTarget,
    reader: &mut R,
    storage_root: &Path,
    capture_pre_install: bool,
) -> Result<RuntimeInstallReport>
where
    R: RuntimeSlotReader + RuntimeSlotWriter + RuntimePageStorer,
{
    if capture_pre_install {
        write_pre_install_bundle(storage_root, bundle, requested_target, reader)?;
    }

    let report = install_runtime_bundle(bundle, requested_target, reader)?;
    replace_directory_copy(
        bundle.root(),
        &installed_runtime_dir_from_root(storage_root),
    )?;
    Ok(report)
}

pub fn install_runtime_with_bundle_dir<R>(
    bundle_dir: impl AsRef<Path>,
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeInstallReport>
where
    R: RuntimeSlotReader + RuntimeSlotWriter + RuntimePageStorer,
{
    let bundle = RuntimeBundle::load_from_dir(bundle_dir)?;
    let storage_root = required_runtime_config_root_dir()?;

    install_runtime_bundle_with_storage(&bundle, requested_target, reader, &storage_root, true)
}

pub fn upgrade_runtime_with_bundle_dir<R>(
    bundle_dir: impl AsRef<Path>,
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeUpgradeReport>
where
    R: RuntimeSlotReader + RuntimeSlotWriter + RuntimePageStorer,
{
    let bundle = RuntimeBundle::load_from_dir(bundle_dir)?;
    let storage_root = required_runtime_config_root_dir()?;
    let install_report = install_runtime_bundle_with_storage(
        &bundle,
        requested_target,
        reader,
        &storage_root,
        false,
    )?;

    Ok(RuntimeUpgradeReport { install_report })
}

pub fn repair_installed_runtime<R>(
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeInstallReport>
where
    R: RuntimeSlotReader + RuntimeSlotWriter + RuntimePageStorer,
{
    let storage_root = required_runtime_config_root_dir()?;
    repair_installed_runtime_with_storage(&storage_root, requested_target, reader)
}

fn repair_installed_runtime_with_storage<R>(
    storage_root: &Path,
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeInstallReport>
where
    R: RuntimeSlotReader + RuntimeSlotWriter + RuntimePageStorer,
{
    let installed_dir = installed_runtime_dir_from_root(storage_root);
    if !installed_dir.is_dir() {
        return Err(RuntimeError::verification_failed(
            "no frozen installed runtime was found under ~/.config/vsn1-cli/runtime",
        ));
    }

    let bundle = RuntimeBundle::load_from_dir(&installed_dir)?;
    let report = inspect_runtime_bundle(&bundle, requested_target, reader)?;
    if report.is_exact_match() {
        return Err(RuntimeError::verification_failed(format!(
            "installed runtime {} is already installed exactly; runtime repair is only for drifted or partial managed content",
            bundle.manifest().bundle_version
        )));
    }

    if report
        .slot_inspections()
        .iter()
        .all(|inspection| matches!(inspection.status, RuntimeSlotStatus::Missing))
    {
        return Err(RuntimeError::verification_failed(
            "no installed runtime content was detected in the owned slots; use `runtime install <name>` for a fresh provision",
        ));
    }

    install_runtime_bundle_with_storage(&bundle, requested_target, reader, storage_root, false)
}

pub fn remove_installed_runtime<R>(
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeRemoveReport>
where
    R: RuntimeSlotReader + RuntimeSlotClearer + RuntimePageStorer + RuntimeSlotWriter,
{
    let storage_root = required_runtime_config_root_dir()?;
    remove_installed_runtime_with_storage(&storage_root, requested_target, reader)
}

fn remove_installed_runtime_with_storage<R>(
    storage_root: &Path,
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeRemoveReport>
where
    R: RuntimeSlotReader + RuntimeSlotClearer + RuntimePageStorer + RuntimeSlotWriter,
{
    let installed_dir = installed_runtime_dir_from_root(storage_root);
    let pre_install_dir = pre_install_runtime_dir_from_root(storage_root);

    let restore_attempt = if pre_install_dir.is_dir() {
        Some(RuntimeBundle::load_from_dir(&pre_install_dir))
    } else {
        None
    };

    let report = match restore_attempt {
        Some(Ok(bundle)) => restore_runtime_bundle(&bundle, requested_target, reader)?,
        Some(Err(_)) => clear_installed_runtime_with_warning(
            &installed_dir,
            requested_target,
            reader,
            "pre-install backup was unavailable or incomplete; owned slots were cleared instead of restored",
        )?,
        None => clear_installed_runtime_with_warning(
            &installed_dir,
            requested_target,
            reader,
            "pre-install backup was unavailable or incomplete; owned slots were cleared instead of restored",
        )?,
    };

    remove_directory_if_exists(&installed_dir)?;
    Ok(report)
}

fn install_runtime_bundle<R>(
    bundle: &RuntimeBundle,
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeInstallReport>
where
    R: RuntimeSlotReader + RuntimeSlotWriter + RuntimePageStorer,
{
    let mut stored_pages = Vec::new();

    for asset in bundle.assets() {
        if !stored_pages.contains(&asset.slot.page) {
            stored_pages.push(asset.slot.page);
        }
        reader.write_owned_slot(requested_target, asset)?;
    }

    for page in stored_pages {
        reader.store_page(requested_target, page)?;
    }

    let verification_report = inspect_runtime_bundle(&bundle, requested_target, reader)?;
    if !verification_report.is_exact_match() {
        return Err(RuntimeError::verification_failed(format!(
            "post-install bundled runtime {} is not an exact match: {}",
            verification_report.bundle_version(),
            verification_report.verification_failure_summary()
        )));
    }

    Ok(RuntimeInstallReport {
        installed_slots: bundle
            .assets()
            .iter()
            .map(|asset| asset.slot.clone())
            .collect(),
        verification_report,
    })
}

fn restore_runtime_bundle<R>(
    bundle: &RuntimeBundle,
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeRemoveReport>
where
    R: RuntimeSlotReader + RuntimeSlotWriter + RuntimePageStorer,
{
    let report = install_runtime_bundle(bundle, requested_target, reader)?;

    Ok(RuntimeRemoveReport {
        removed_slots: report.installed_slots().to_vec(),
        restored_from_backup: true,
        warning: None,
    })
}

fn clear_installed_runtime_with_warning<R>(
    installed_dir: &Path,
    requested_target: ResolvedTarget,
    reader: &mut R,
    warning: &str,
) -> Result<RuntimeRemoveReport>
where
    R: RuntimeSlotReader + RuntimeSlotClearer + RuntimePageStorer,
{
    if !installed_dir.is_dir() {
        return Err(RuntimeError::verification_failed(
            "no frozen installed runtime was found under ~/.config/vsn1-cli/runtime, so owned slots could not be restored or cleared",
        ));
    }

    let bundle = RuntimeBundle::load_from_dir(installed_dir)?;
    let mut stored_pages = Vec::new();

    for asset in bundle.assets() {
        if !stored_pages.contains(&asset.slot.page) {
            stored_pages.push(asset.slot.page);
        }
        reader.clear_owned_slot(requested_target, &asset.slot)?;
    }

    for page in stored_pages {
        reader.store_page(requested_target, page)?;
    }

    let removed_slots = bundle
        .assets()
        .iter()
        .map(|asset| asset.slot.clone())
        .collect::<Vec<_>>();
    verify_removed_slots(requested_target, &removed_slots, reader)?;

    Ok(RuntimeRemoveReport {
        removed_slots,
        restored_from_backup: false,
        warning: Some(warning.to_string()),
    })
}

fn write_pre_install_bundle<R>(
    storage_root: &Path,
    bundle: &RuntimeBundle,
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<()>
where
    R: RuntimeSlotReader,
{
    let backup_dir = pre_install_runtime_dir_from_root(storage_root);
    let staging_dir = staging_dir_for(&backup_dir);

    remove_directory_if_exists(&staging_dir)?;
    fs::create_dir_all(&staging_dir).map_err(|error| {
        RuntimeError::host_storage(format!(
            "failed to create pre-install staging directory {}: {error}",
            staging_dir.display()
        ))
    })?;

    for asset in bundle.assets() {
        let content = read_slot_content_for_backup(requested_target, &asset.slot, reader)?;
        let asset_path = staging_dir.join(&asset.slot.asset);
        fs::write(&asset_path, content).map_err(|error| {
            RuntimeError::host_storage(format!(
                "failed to write pre-install asset {}: {error}",
                asset_path.display()
            ))
        })?;
    }

    let manifest = StoredRuntimeManifest {
        bundle_version: format!(
            "pre-install-backup-from-{}",
            bundle.manifest().bundle_version
        ),
        compatibility_reference: "VSN1 pre-install backup captured by vsn1-cli".to_string(),
        runtime_marker: "vsn1-cli:pre-install-backup".to_string(),
        compatibility_notes: vec![format!(
            "captured from requested target {} before runtime install",
            requested_target
        )],
        layers: bundle.manifest().layers.clone(),
        owned_slots: bundle
            .assets()
            .iter()
            .map(|asset| StoredOwnedRuntimeSlot {
                name: asset.slot.name.clone(),
                page: asset.slot.page,
                element: asset.slot.element,
                event: asset.slot.event,
                asset: asset.slot.asset.clone(),
                install_order: asset.slot.install_order,
                runtime_marker: format!("vsn1-cli:pre-install-backup:{}", asset.slot.name),
            })
            .collect(),
    };
    let manifest_path = staging_dir.join("manifest.toml");
    fs::write(
        &manifest_path,
        toml::to_string(&manifest).map_err(|error| {
            RuntimeError::host_storage(format!(
                "failed to serialize pre-install manifest {}: {error}",
                manifest_path.display()
            ))
        })?,
    )
    .map_err(|error| {
        RuntimeError::host_storage(format!(
            "failed to write pre-install manifest {}: {error}",
            manifest_path.display()
        ))
    })?;

    replace_staged_directory(&staging_dir, &backup_dir)
}

fn read_slot_content_for_backup<R>(
    requested_target: ResolvedTarget,
    slot: &OwnedRuntimeSlot,
    reader: &mut R,
) -> Result<String>
where
    R: RuntimeSlotReader,
{
    let Some(read) = reader.read_owned_slot(requested_target, slot)? else {
        return Ok(String::new());
    };

    if let ResolvedTarget::Explicit(expected_target) = requested_target {
        if read.source_target != expected_target {
            return Err(RuntimeError::verification_failed(format!(
                "refusing to capture pre-install backup for {} at {} because it responded from dx={} dy={} instead of the requested target",
                slot.name,
                slot.location_display(),
                read.source_target.dx,
                read.source_target.dy
            )));
        }
    }

    Ok(normalize_text_content(&read.content))
}

fn replace_directory_copy(source: &Path, destination: &Path) -> Result<()> {
    let staging_dir = staging_dir_for(destination);

    remove_directory_if_exists(&staging_dir)?;
    copy_directory_recursive(source, &staging_dir)?;
    replace_staged_directory(&staging_dir, destination)
}

fn copy_directory_recursive(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination).map_err(|error| {
        RuntimeError::host_storage(format!(
            "failed to create directory {}: {error}",
            destination.display()
        ))
    })?;

    for entry in fs::read_dir(source).map_err(|error| {
        RuntimeError::host_storage(format!(
            "failed to read directory {}: {error}",
            source.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            RuntimeError::host_storage(format!(
                "failed to read directory entry in {}: {error}",
                source.display()
            ))
        })?;
        let entry_path = entry.path();
        let destination_path = destination.join(entry.file_name());

        if entry_path.is_dir() {
            copy_directory_recursive(&entry_path, &destination_path)?;
        } else {
            fs::copy(&entry_path, &destination_path).map_err(|error| {
                RuntimeError::host_storage(format!(
                    "failed to copy {} to {}: {error}",
                    entry_path.display(),
                    destination_path.display()
                ))
            })?;
        }
    }

    Ok(())
}

fn replace_staged_directory(staging_dir: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            RuntimeError::host_storage(format!(
                "failed to create directory {}: {error}",
                parent.display()
            ))
        })?;
    }

    remove_directory_if_exists(destination)?;
    fs::rename(staging_dir, destination).map_err(|error| {
        RuntimeError::host_storage(format!(
            "failed to move {} to {}: {error}",
            staging_dir.display(),
            destination.display()
        ))
    })
}

fn remove_directory_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|error| {
            RuntimeError::host_storage(format!(
                "failed to remove directory {}: {error}",
                path.display()
            ))
        })?;
    }

    Ok(())
}

fn staging_dir_for(path: &Path) -> PathBuf {
    path.with_extension("tmp")
}

fn verify_removed_slots<R>(
    requested_target: ResolvedTarget,
    removed_slots: &[OwnedRuntimeSlot],
    reader: &mut R,
) -> Result<()>
where
    R: RuntimeSlotReader,
{
    let cleared_slot_sha256 = normalized_sha256(&protocol::frame_lua(""));

    for slot in removed_slots {
        if let Some(read) = reader.read_owned_slot(requested_target, slot)? {
            if let ResolvedTarget::Explicit(expected_target) = requested_target {
                if read.source_target != expected_target {
                    return Err(RuntimeError::verification_failed(format!(
                        "post-remove owned slot {} at {} responded from dx={} dy={} instead of the requested target",
                        slot.name,
                        slot.location_display(),
                        read.source_target.dx,
                        read.source_target.dy
                    )));
                }
            }

            if normalized_sha256(&read.content) == cleared_slot_sha256 {
                continue;
            }

            return Err(RuntimeError::verification_failed(format!(
                "post-remove owned slot {} at {} still contains content on dx={} dy={}",
                slot.name,
                slot.location_display(),
                read.source_target.dx,
                read.source_target.dy
            )));
        }
    }

    Ok(())
}

fn inspect_runtime_bundle<R>(
    bundle: &RuntimeBundle,
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeInspectionReport>
where
    R: RuntimeSlotReader,
{
    let mut slot_inspections = Vec::with_capacity(bundle.assets().len());
    let mut observed_targets = Vec::new();

    for asset in bundle.assets() {
        let read = reader.read_owned_slot(requested_target, &asset.slot)?;

        let status = match read {
            None => RuntimeSlotStatus::Missing,
            Some(read) => {
                push_unique_target(&mut observed_targets, read.source_target);

                match requested_target {
                    ResolvedTarget::Explicit(expected) if read.source_target != expected => {
                        RuntimeSlotStatus::WrongTarget {
                            actual_target: read.source_target,
                        }
                    }
                    _ => {
                        if normalize_text_content(&read.content) == asset.stored_content {
                            RuntimeSlotStatus::Match {
                                source_target: read.source_target,
                            }
                        } else {
                            RuntimeSlotStatus::Drifted {
                                source_target: read.source_target,
                            }
                        }
                    }
                }
            }
        };

        slot_inspections.push(RuntimeSlotInspection {
            slot: asset.slot.clone(),
            status,
        });
    }

    Ok(RuntimeInspectionReport {
        bundle_version: bundle.manifest().bundle_version.clone(),
        requested_target,
        observed_targets,
        slot_inspections,
    })
}

fn read_transport_until_idle(
    transport: &mut impl SerialTransport,
    total_window: Duration,
    idle_window: Duration,
) -> Result<Vec<u8>> {
    let started_at = std::time::Instant::now();
    let mut last_data_at: Option<std::time::Instant> = None;
    let mut inbound = Vec::new();
    let mut buffer = [0u8; 1024];

    loop {
        if started_at.elapsed() >= total_window {
            break;
        }

        if let Some(last_data_at) = last_data_at {
            if last_data_at.elapsed() >= idle_window {
                break;
            }
        }

        let available = transport.bytes_to_read()? as usize;
        if available == 0 {
            thread::sleep(READ_BACK_POLL_INTERVAL);
            continue;
        }

        let target_len = available.min(buffer.len());
        let read_len = transport.read(&mut buffer[..target_len])?;
        if read_len == 0 {
            thread::sleep(READ_BACK_POLL_INTERVAL);
            continue;
        }

        inbound.extend_from_slice(&buffer[..read_len]);
        last_data_at = Some(std::time::Instant::now());
    }

    Ok(inbound)
}

fn extract_single_config_report(
    inbound: &[u8],
    slot: &OwnedRuntimeSlot,
) -> Result<Option<ConfigFetchReport>> {
    let mut reports = Vec::new();

    for frame in split_complete_frames(inbound) {
        if !verify_grid_frame_checksum(frame) {
            continue;
        }

        let Some(source_x) = parse_grid_coordinate_range(frame, 10) else {
            continue;
        };
        let Some(source_y) = parse_grid_coordinate_range(frame, 12) else {
            continue;
        };
        let source_target = GridTarget::new(source_x, source_y);

        for block in split_class_blocks(frame) {
            if parse_ascii_hex_range(block, 1, 3) != Some(GRID_CLASS_CONFIG) {
                continue;
            }

            if parse_ascii_hex_range(block, 4, 1) != Some(GRID_INSTR_REPORT) {
                continue;
            }

            if parse_ascii_hex_range(block, 11, 2) != Some(slot.page as usize)
                || parse_ascii_hex_range(block, 13, 2) != Some(slot.element as usize)
                || parse_ascii_hex_range(block, 15, 2) != Some(slot.event as usize)
            {
                continue;
            }

            let content = parse_ascii_text_range(block, 21, block.len().saturating_sub(22));
            reports.push(ConfigFetchReport {
                source_target,
                content,
            });
        }
    }

    let mut unique_reports = Vec::new();
    for report in reports {
        if !unique_reports.iter().any(|existing| existing == &report) {
            unique_reports.push(report);
        }
    }

    match unique_reports.len() {
        0 => Ok(None),
        1 => {
            let report = unique_reports.pop().unwrap();
            if report.content.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(report))
            }
        }
        _ => Err(RuntimeError::unexpected_response(format!(
            "multiple config reports were returned for {} at {} ({})",
            slot.name,
            slot.location_display(),
            format_targets(
                &unique_reports
                    .iter()
                    .map(|report| report.source_target)
                    .collect::<Vec<_>>()
            )
        ))),
    }
}

fn extract_page_store_ack(inbound: &[u8], requested_target: ResolvedTarget) -> Result<()> {
    let mut acknowledgements = Vec::new();
    let mut rejections = Vec::new();

    for frame in split_complete_frames(inbound) {
        if !verify_grid_frame_checksum(frame) {
            continue;
        }

        let Some(source_x) = parse_grid_coordinate_range(frame, 10) else {
            continue;
        };
        let Some(source_y) = parse_grid_coordinate_range(frame, 12) else {
            continue;
        };
        let source_target = GridTarget::new(source_x, source_y);

        for block in split_class_blocks(frame) {
            if parse_ascii_hex_range(block, 1, 3) != Some(GRID_CLASS_PAGESTORE) {
                continue;
            }

            match parse_ascii_hex_range(block, 4, 1) {
                Some(GRID_INSTR_ACKNOWLEDGE) => acknowledgements.push(source_target),
                Some(GRID_INSTR_NACKNOWLEDGE) => rejections.push(source_target),
                _ => {}
            }
        }
    }

    if let Some(actual_target) = rejections.into_iter().next() {
        return Err(RuntimeError::verification_failed(format!(
            "page store was rejected by dx={} dy={}",
            actual_target.dx, actual_target.dy
        )));
    }

    match acknowledgements.as_slice() {
        [] => Err(RuntimeError::unexpected_response(
            "no PAGESTORE acknowledge was observed in read-back",
        )),
        [actual_target] => match requested_target {
            ResolvedTarget::Explicit(expected) if *actual_target != expected => {
                Err(RuntimeError::verification_failed(format!(
                    "page store acknowledged from dx={} dy={} instead of the requested target",
                    actual_target.dx, actual_target.dy
                )))
            }
            _ => Ok(()),
        },
        targets => Err(RuntimeError::unexpected_response(format!(
            "multiple PAGESTORE acknowledgements were returned ({})",
            format_targets(targets)
        ))),
    }
}

fn split_complete_frames(bytes: &[u8]) -> Vec<&[u8]> {
    let mut frames = Vec::new();
    let mut frame_start = 0;

    for index in 3..bytes.len() {
        if bytes[index] == GRID_CONST_LF && bytes[index - 3] == GRID_CONST_EOT {
            frames.push(&bytes[frame_start..=index]);
            frame_start = index + 1;
        }
    }

    frames
}

fn split_class_blocks(frame: &[u8]) -> Vec<&[u8]> {
    if frame.len() < 27 {
        return Vec::new();
    }

    let Some(class_region) = frame.get(23..frame.len().saturating_sub(4)) else {
        return Vec::new();
    };

    let mut blocks = Vec::new();
    let mut start = 0;
    for index in 0..class_region.len() {
        if class_region[index] == GRID_CONST_ETX {
            blocks.push(&class_region[start..=index]);
            start = index + 1;
        }
    }

    blocks
}

fn verify_grid_frame_checksum(frame: &[u8]) -> bool {
    if frame.len() < 4 {
        return false;
    }

    let Some(received_checksum) = parse_ascii_hex_range(frame, frame.len() - 3, 2) else {
        return false;
    };

    let calculated_checksum = frame[..frame.len() - 3]
        .iter()
        .fold(0u8, |acc, byte| acc ^ byte) as usize;

    received_checksum == calculated_checksum
}

fn parse_grid_coordinate_range(frame: &[u8], offset: usize) -> Option<i16> {
    let raw = parse_ascii_hex_range(frame, offset, 2)? as i16;
    Some(raw - 127)
}

fn parse_ascii_hex_range(frame: &[u8], offset: usize, width: usize) -> Option<usize> {
    let slice = frame.get(offset..offset + width)?;
    let text = std::str::from_utf8(slice).ok()?;
    usize::from_str_radix(text, 16).ok()
}

fn parse_ascii_text_range(frame: &[u8], offset: usize, width: usize) -> String {
    let Some(slice) = frame.get(offset..offset + width) else {
        return String::new();
    };

    String::from_utf8_lossy(slice).into_owned()
}

fn push_unique_target(targets: &mut Vec<GridTarget>, target: GridTarget) {
    if !targets.iter().any(|existing| existing == &target) {
        targets.push(target);
        targets.sort_by_key(|target| (target.dx, target.dy));
    }
}

pub fn installed_runtime_dir() -> Option<PathBuf> {
    runtime_config_root_dir().map(|root| installed_runtime_dir_from_root(&root))
}

pub fn pre_install_runtime_dir() -> Option<PathBuf> {
    runtime_config_root_dir().map(|root| pre_install_runtime_dir_from_root(&root))
}

fn runtime_config_root_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .map(|home| home.join(".config/vsn1-cli"))
}

fn required_runtime_config_root_dir() -> Result<PathBuf> {
    runtime_config_root_dir().ok_or_else(|| {
        RuntimeError::host_storage(
            "home directory is unavailable; cannot persist runtime state under ~/.config/vsn1-cli",
        )
    })
}

fn installed_runtime_dir_from_root(root: &Path) -> PathBuf {
    root.join("runtime")
}

fn pre_install_runtime_dir_from_root(root: &Path) -> PathBuf {
    root.join("pre-install")
}

fn format_targets(targets: &[GridTarget]) -> String {
    targets
        .iter()
        .map(|target| format!("dx={} dy={}", target.dx, target.dy))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use crate::protocol::frame_lua;
    use crate::runtime_bundle::normalize_text_content;

    #[derive(Default)]
    struct StaticSlotReader {
        slots: BTreeMap<String, RuntimeSlotRead>,
    }

    impl StaticSlotReader {
        fn insert(&mut self, slot: &OwnedRuntimeSlot, source_target: GridTarget, content: String) {
            self.slots.insert(
                slot.name.clone(),
                RuntimeSlotRead {
                    source_target,
                    content,
                },
            );
        }
    }

    impl RuntimeSlotReader for StaticSlotReader {
        fn read_owned_slot(
            &mut self,
            _target: ResolvedTarget,
            slot: &OwnedRuntimeSlot,
        ) -> Result<Option<RuntimeSlotRead>> {
            Ok(self.slots.get(&slot.name).cloned())
        }
    }

    #[derive(Default)]
    struct RecordingSlotAccessor {
        writes: Vec<String>,
        clears: Vec<String>,
        stored_pages: Vec<u8>,
        slots: BTreeMap<String, RuntimeSlotRead>,
        persist_writes: bool,
        drifted_slot: Option<String>,
        reject_page_store: bool,
    }

    impl RecordingSlotAccessor {
        fn write_order(&self) -> &[String] {
            &self.writes
        }

        fn stored_pages(&self) -> &[u8] {
            &self.stored_pages
        }

        fn clear_order(&self) -> &[String] {
            &self.clears
        }
    }

    impl RuntimeSlotReader for RecordingSlotAccessor {
        fn read_owned_slot(
            &mut self,
            _target: ResolvedTarget,
            slot: &OwnedRuntimeSlot,
        ) -> Result<Option<RuntimeSlotRead>> {
            Ok(self.slots.get(&slot.name).cloned())
        }
    }

    impl RuntimeSlotWriter for RecordingSlotAccessor {
        fn write_owned_slot(&mut self, target: ResolvedTarget, asset: &RuntimeAsset) -> Result<()> {
            self.writes.push(asset.slot.name.clone());

            if self.persist_writes {
                let source_target = match target {
                    ResolvedTarget::Broadcast => GridTarget::new(0, 0),
                    ResolvedTarget::Explicit(target) => target,
                };
                let content = if self.drifted_slot.as_deref() == Some(asset.slot.name.as_str()) {
                    normalize_text_content(&frame_lua(&format!(
                        "{}\n-- drifted after install\n",
                        asset.slot.name
                    )))
                } else {
                    asset.stored_content.clone()
                };

                self.slots.insert(
                    asset.slot.name.clone(),
                    RuntimeSlotRead {
                        source_target,
                        content,
                    },
                );
            }

            Ok(())
        }
    }

    impl RuntimePageStorer for RecordingSlotAccessor {
        fn store_page(&mut self, _target: ResolvedTarget, page: u8) -> Result<()> {
            self.stored_pages.push(page);

            if self.reject_page_store {
                Err(RuntimeError::verification_failed(
                    "page store was rejected by dx=0 dy=0",
                ))
            } else {
                Ok(())
            }
        }
    }

    impl RuntimeSlotClearer for RecordingSlotAccessor {
        fn clear_owned_slot(
            &mut self,
            _target: ResolvedTarget,
            slot: &OwnedRuntimeSlot,
        ) -> Result<()> {
            self.clears.push(slot.name.clone());
            self.slots.insert(
                slot.name.clone(),
                RuntimeSlotRead {
                    source_target: GridTarget::new(0, 0),
                    content: normalize_text_content(&frame_lua("")),
                },
            );
            Ok(())
        }
    }

    fn read_backup_bundle(root: &Path) -> RuntimeBundle {
        RuntimeBundle::load_from_dir(pre_install_runtime_dir_from_root(root)).unwrap()
    }

    fn write_runtime_bundle_dir(runtime_root: &Path, bundle_version: &str, draw_content: &str) {
        let init_content = "return 'init'\n";

        fs::create_dir_all(runtime_root).unwrap();
        fs::write(runtime_root.join("lcd-init.lua"), init_content).unwrap();
        fs::write(runtime_root.join("lcd-draw.lua"), draw_content).unwrap();
        fs::write(
            runtime_root.join("manifest.toml"),
            format!(
                r#"
bundle_version = "{bundle_version}"
compatibility_reference = "fixture"
runtime_marker = "fixture"

[[layers]]
name = "persistent"
priority = 0
activation = "persistent"

[[owned_slots]]
name = "lcd-init"
page = 0
element = 13
event = 0
asset = "lcd-init.lua"
install_order = 10
runtime_marker = "fixture:lcd-init"

[[owned_slots]]
name = "lcd-draw"
page = 0
element = 13
event = 8
asset = "lcd-draw.lua"
install_order = 20
runtime_marker = "fixture:lcd-draw"
"#,
            ),
        )
        .unwrap();
    }

    fn write_runtime_fixture(root: &Path, name: &str, bundle_version: &str, draw_content: &str) {
        write_runtime_bundle_dir(&root.join(name), bundle_version, draw_content);
    }

    #[test]
    fn inspect_reports_exact_match_when_all_owned_slots_match() {
        let bundle = RuntimeBundle::bundled().unwrap();
        let mut reader = StaticSlotReader::default();

        for asset in bundle.assets() {
            reader.insert(
                &asset.slot,
                GridTarget::new(0, 0),
                asset.stored_content.clone(),
            );
        }

        let report =
            inspect_bundled_runtime(ResolvedTarget::Explicit(GridTarget::new(0, 0)), &mut reader)
                .unwrap();

        assert!(report.is_exact_match());
        assert_eq!(report.status_label(), "exact-match compatible");
        assert_eq!(report.observed_target(), Some(GridTarget::new(0, 0)));
        assert!(report
            .slot_inspections()
            .iter()
            .all(|inspection| matches!(inspection.status, RuntimeSlotStatus::Match { .. })));
    }

    #[test]
    fn verify_runtime_with_bundle_dir_fails_when_owned_slot_content_drifted() {
        let bundle = RuntimeBundle::bundled().unwrap();
        let mut reader = StaticSlotReader::default();

        for asset in bundle.assets() {
            let content = if asset.slot.name == "lcd-draw" {
                normalize_text_content(&frame_lua("return 'drifted'\n"))
            } else {
                asset.stored_content.clone()
            };

            reader.insert(&asset.slot, GridTarget::new(0, 0), content);
        }

        let error = verify_runtime_with_bundle_dir(
            crate::runtime_bundle::bundled_runtime_dir(),
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut reader,
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("lcd-draw at page=0 element=13 event=8 drifted"));
    }

    #[test]
    fn inspect_installed_runtime_reports_none_when_no_local_copy_exists() {
        let report = inspect_runtime_with_optional_bundle_dir(
            None,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut StaticSlotReader::default(),
        )
        .unwrap();

        assert!(report.is_none());
    }

    #[test]
    fn verify_installed_runtime_requires_local_copy() {
        let error = verify_runtime_with_optional_bundle_dir(
            None,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut StaticSlotReader::default(),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "runtime verification failed: no frozen installed runtime was found under ~/.config/vsn1-cli/runtime"
        );
    }

    #[test]
    fn inspect_runtime_with_optional_bundle_dir_uses_frozen_runtime_copy_when_present() {
        let fixture = tempdir().unwrap();
        write_runtime_fixture(
            fixture.path(),
            "runtime",
            "frozen-runtime",
            "return 'draw'\n",
        );
        let bundle = RuntimeBundle::load_from_dir(fixture.path().join("runtime")).unwrap();
        let mut reader = StaticSlotReader::default();

        for asset in bundle.assets() {
            reader.insert(
                &asset.slot,
                GridTarget::new(0, 0),
                asset.stored_content.clone(),
            );
        }

        let report = inspect_runtime_with_optional_bundle_dir(
            Some(fixture.path().join("runtime").as_path()),
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut reader,
        )
        .unwrap()
        .unwrap();

        assert_eq!(report.bundle_version(), "frozen-runtime");
        assert!(report.is_exact_match());
    }

    #[test]
    fn inspect_marks_missing_owned_slots() {
        let bundle = RuntimeBundle::bundled().unwrap();
        let mut reader = StaticSlotReader::default();

        let init_asset = &bundle.assets()[0];
        reader.insert(
            &init_asset.slot,
            GridTarget::new(0, 0),
            init_asset.stored_content.clone(),
        );

        let report =
            inspect_bundled_runtime(ResolvedTarget::Explicit(GridTarget::new(0, 0)), &mut reader)
                .unwrap();

        assert!(!report.is_exact_match());
        assert!(report
            .slot_inspections()
            .iter()
            .any(|inspection| inspection.slot.name == "lcd-draw"
                && matches!(inspection.status, RuntimeSlotStatus::Missing)));
    }

    #[test]
    fn inspect_surfaces_malformed_manifest_errors() {
        let fixture = tempdir().unwrap();
        fs::write(
            fixture.path().join("manifest.toml"),
            r#"
bundle_version = "broken"
compatibility_reference = "fixture"
runtime_marker = "fixture"
layers = [{ name = "persistent", priority = 0, activation = "persistent" }]
owned_slots = []
"#,
        )
        .unwrap();

        let error = inspect_runtime_with_bundle_dir(
            fixture.path(),
            ResolvedTarget::Broadcast,
            &mut StaticSlotReader::default(),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid runtime manifest: owned_slots must not be empty"
        );
    }

    #[test]
    fn install_uses_manifest_order_and_verifies_the_written_bundle() {
        let fixture = tempdir().unwrap();
        let storage = tempdir().unwrap();
        let root = fixture.path();
        let config_root = storage.path().join("config");
        let first_content = "return 'first'\n";
        let second_content = "return 'second'\n";

        fs::write(root.join("first.lua"), first_content).unwrap();
        fs::write(root.join("second.lua"), second_content).unwrap();
        fs::write(
            root.join("manifest.toml"),
            format!(
                r#"
bundle_version = "test-install"
compatibility_reference = "fixture"
runtime_marker = "fixture"

[[layers]]
name = "persistent"
priority = 0
activation = "persistent"

[[owned_slots]]
name = "second"
page = 0
element = 13
event = 8
asset = "second.lua"
install_order = 20
runtime_marker = "fixture:second"

[[owned_slots]]
name = "first"
page = 0
element = 13
event = 0
asset = "first.lua"
install_order = 10
runtime_marker = "fixture:first"
"#,
            ),
        )
        .unwrap();

        let mut accessor = RecordingSlotAccessor {
            persist_writes: true,
            ..Default::default()
        };
        accessor.slots.insert(
            "first".to_string(),
            RuntimeSlotRead {
                source_target: GridTarget::new(0, 0),
                content: normalize_text_content(&frame_lua("return 'pre-first'\n")),
            },
        );

        let bundle = RuntimeBundle::load_from_dir(root).unwrap();
        let report = install_runtime_bundle_with_storage(
            &bundle,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut accessor,
            &config_root,
            true,
        )
        .unwrap();

        assert_eq!(
            accessor.write_order(),
            &["first".to_string(), "second".to_string()]
        );
        assert_eq!(accessor.stored_pages(), &[0]);
        assert_eq!(
            report
                .installed_slots()
                .iter()
                .map(|slot| slot.name.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        assert!(report.verification_report().is_exact_match());

        let installed_bundle =
            RuntimeBundle::load_from_dir(installed_runtime_dir_from_root(&config_root)).unwrap();
        assert_eq!(installed_bundle.manifest().bundle_version, "test-install");

        let backup_bundle = read_backup_bundle(&config_root);
        assert_eq!(backup_bundle.assets().len(), 2);
        assert_eq!(backup_bundle.assets()[0].slot.name, "first");
        assert_eq!(
            backup_bundle.assets()[0].stored_content,
            normalize_text_content(&frame_lua("return 'pre-first'\n"))
        );
        assert_eq!(backup_bundle.assets()[1].slot.name, "second");
        assert_eq!(backup_bundle.assets()[1].normalized_content, "");
    }

    #[test]
    fn install_fails_when_page_store_is_rejected() {
        let fixture = tempdir().unwrap();
        let mut accessor = RecordingSlotAccessor {
            reject_page_store: true,
            ..Default::default()
        };

        let bundle = RuntimeBundle::bundled().unwrap();
        let error = install_runtime_bundle_with_storage(
            &bundle,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut accessor,
            fixture.path(),
            true,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "runtime verification failed: page store was rejected by dx=0 dy=0"
        );
        assert_eq!(accessor.stored_pages(), &[0]);
    }

    #[test]
    fn install_fails_when_post_install_verification_still_drifted() {
        let fixture = tempdir().unwrap();
        let mut accessor = RecordingSlotAccessor {
            persist_writes: true,
            drifted_slot: Some("lcd-draw".to_string()),
            ..Default::default()
        };

        let bundle = RuntimeBundle::bundled().unwrap();
        let error = install_runtime_bundle_with_storage(
            &bundle,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut accessor,
            fixture.path(),
            true,
        )
        .unwrap_err();

        assert!(error.to_string().contains(
            "post-install bundled runtime 2026-06-21-manifest-layers.1 is not an exact match"
        ));
        assert!(error
            .to_string()
            .contains("lcd-draw at page=0 element=13 event=8 drifted"));
    }

    #[test]
    fn upgrade_overwrites_device_without_refreshing_pre_install_backup() {
        let fixture = tempdir().unwrap();
        let storage = tempdir().unwrap();
        write_runtime_fixture(
            fixture.path(),
            "current",
            "2026-06-21-manifest-layers.1",
            "return 'current draw'\n",
        );
        write_runtime_fixture(
            fixture.path(),
            "older",
            "2026-06-17-screen-first.7",
            "return 'older draw'\n",
        );

        let current_bundle = RuntimeBundle::load_from_dir(fixture.path().join("current")).unwrap();
        let older_bundle = RuntimeBundle::load_from_dir(fixture.path().join("older")).unwrap();
        let mut accessor = RecordingSlotAccessor {
            persist_writes: true,
            ..Default::default()
        };

        for asset in older_bundle.assets() {
            accessor.slots.insert(
                asset.slot.name.clone(),
                RuntimeSlotRead {
                    source_target: GridTarget::new(0, 0),
                    content: asset.stored_content.clone(),
                },
            );
        }

        fs::create_dir_all(pre_install_runtime_dir_from_root(storage.path())).unwrap();
        fs::write(
            pre_install_runtime_dir_from_root(storage.path()).join("sentinel.txt"),
            "keep",
        )
        .unwrap();

        let report = install_runtime_bundle_with_storage(
            &current_bundle,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut accessor,
            storage.path(),
            false,
        )
        .unwrap();

        assert_eq!(
            accessor.write_order(),
            &current_bundle
                .assets()
                .iter()
                .map(|asset| asset.slot.name.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(accessor.stored_pages(), &[0]);
        assert!(report.verification_report().is_exact_match());
        let installed_bundle =
            RuntimeBundle::load_from_dir(installed_runtime_dir_from_root(storage.path())).unwrap();
        assert_eq!(
            installed_bundle.manifest().bundle_version,
            current_bundle.manifest().bundle_version
        );
        assert!(pre_install_runtime_dir_from_root(storage.path())
            .join("sentinel.txt")
            .exists());
    }

    #[test]
    fn repair_reinstalls_current_bundle_when_owned_slots_are_drifted_or_missing() {
        let fixture = tempdir().unwrap();
        let bundle = RuntimeBundle::bundled().unwrap();
        replace_directory_copy(
            bundle.root(),
            &installed_runtime_dir_from_root(fixture.path()),
        )
        .unwrap();
        let mut accessor = RecordingSlotAccessor {
            persist_writes: true,
            ..Default::default()
        };

        for asset in bundle.assets() {
            let content = if asset.slot.name == "lcd-init" {
                normalize_text_content(&frame_lua("return 'drifted'\n"))
            } else {
                String::new()
            };

            if !content.is_empty() {
                accessor.slots.insert(
                    asset.slot.name.clone(),
                    RuntimeSlotRead {
                        source_target: GridTarget::new(0, 0),
                        content,
                    },
                );
            }
        }

        let report = repair_installed_runtime_with_storage(
            fixture.path(),
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut accessor,
        )
        .unwrap();

        assert_eq!(
            accessor.write_order(),
            &bundle
                .assets()
                .iter()
                .map(|asset| asset.slot.name.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(accessor.stored_pages(), &[0]);
        assert!(report.verification_report().is_exact_match());
        let installed_bundle =
            RuntimeBundle::load_from_dir(installed_runtime_dir_from_root(fixture.path())).unwrap();
        assert_eq!(
            installed_bundle.manifest().bundle_version,
            bundle.manifest().bundle_version
        );
    }

    #[test]
    fn install_replaces_existing_frozen_runtime_and_backup_directories() {
        let fixture = tempdir().unwrap();
        let config_root = fixture.path().join("config");
        let bundle = RuntimeBundle::bundled().unwrap();
        fs::create_dir_all(installed_runtime_dir_from_root(&config_root)).unwrap();
        fs::write(
            installed_runtime_dir_from_root(&config_root).join("stale.txt"),
            "stale",
        )
        .unwrap();
        fs::create_dir_all(pre_install_runtime_dir_from_root(&config_root)).unwrap();
        fs::write(
            pre_install_runtime_dir_from_root(&config_root).join("stale.txt"),
            "stale",
        )
        .unwrap();

        let mut accessor = RecordingSlotAccessor {
            persist_writes: true,
            ..Default::default()
        };
        let report = install_runtime_bundle_with_storage(
            &bundle,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut accessor,
            &config_root,
            true,
        )
        .unwrap();

        assert!(report.verification_report().is_exact_match());
        assert!(!installed_runtime_dir_from_root(&config_root)
            .join("stale.txt")
            .exists());
        assert!(!pre_install_runtime_dir_from_root(&config_root)
            .join("stale.txt")
            .exists());
    }

    #[test]
    fn remove_restores_pre_install_backup_when_available() {
        let fixture = tempdir().unwrap();
        let bundle = RuntimeBundle::bundled().unwrap();
        write_runtime_bundle_dir(
            &installed_runtime_dir_from_root(fixture.path()),
            &bundle.manifest().bundle_version,
            "return 'installed draw'\n",
        );
        write_runtime_bundle_dir(
            &pre_install_runtime_dir_from_root(fixture.path()),
            "pre-install-backup",
            "return 'backup draw'\n",
        );
        let mut accessor = RecordingSlotAccessor {
            persist_writes: true,
            ..Default::default()
        };

        for asset in bundle.assets() {
            accessor.slots.insert(
                asset.slot.name.clone(),
                RuntimeSlotRead {
                    source_target: GridTarget::new(0, 0),
                    content: asset.stored_content.clone(),
                },
            );
        }

        let report = remove_installed_runtime_with_storage(
            fixture.path(),
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut accessor,
        )
        .unwrap();

        assert_eq!(
            accessor.write_order(),
            &bundle
                .assets()
                .iter()
                .map(|asset| asset.slot.name.clone())
                .collect::<Vec<_>>()
        );
        assert!(accessor.clear_order().is_empty());
        assert_eq!(accessor.stored_pages(), &[0]);
        assert_eq!(report.removed_slots().len(), bundle.assets().len());
        assert!(report.restored_from_backup());
        assert_eq!(report.warning(), None);
        assert!(!installed_runtime_dir_from_root(fixture.path()).exists());
        assert!(accessor.slots.values().any(
            |slot| slot.content == normalize_text_content(&frame_lua("return 'backup draw'\n"))
        ));
    }

    #[test]
    fn remove_clears_owned_slots_with_warning_when_backup_is_missing() {
        let fixture = tempdir().unwrap();
        let bundle = RuntimeBundle::bundled().unwrap();
        write_runtime_bundle_dir(
            &installed_runtime_dir_from_root(fixture.path()),
            &bundle.manifest().bundle_version,
            "return 'installed draw'\n",
        );
        let mut accessor = RecordingSlotAccessor::default();

        for asset in bundle.assets() {
            accessor.slots.insert(
                asset.slot.name.clone(),
                RuntimeSlotRead {
                    source_target: GridTarget::new(0, 0),
                    content: asset.stored_content.clone(),
                },
            );
        }

        let report = remove_installed_runtime_with_storage(
            fixture.path(),
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut accessor,
        )
        .unwrap();

        assert!(!report.restored_from_backup());
        assert!(report
            .warning()
            .unwrap()
            .contains("pre-install backup was unavailable or incomplete"));
        assert_eq!(
            accessor.clear_order(),
            &bundle
                .assets()
                .iter()
                .map(|asset| asset.slot.name.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(accessor.stored_pages(), &[0]);
        assert!(!installed_runtime_dir_from_root(fixture.path()).exists());
        assert!(accessor
            .slots
            .values()
            .all(|slot| normalized_sha256(&slot.content) == normalized_sha256(&frame_lua(""))));
    }
}
