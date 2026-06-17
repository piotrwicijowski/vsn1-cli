use std::error::Error as StdError;
use std::fmt;
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::protocol::{
    self, ConfigFetch, ConfigLocation, ConfigWrite, GridTarget, Heartbeat, PacketIdentity,
    PageActive, PageStore, ProtocolError,
};
use crate::runtime_bundle::{
    normalized_sha256, OwnedRuntimeSlot, RuntimeAsset, RuntimeBundle, RuntimeBundleError,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeSlotStatus {
    Match {
        source_target: GridTarget,
    },
    Missing,
    Drifted {
        actual_sha256: String,
        source_target: GridTarget,
    },
    WrongTarget {
        actual_target: GridTarget,
    },
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
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bundle(error) => error.fmt(f),
            Self::Protocol(error) => error.fmt(f),
            Self::Transport(error) => error.fmt(f),
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
            Self::UnexpectedResponse { .. } | Self::VerificationFailed { .. } => None,
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
                RuntimeSlotStatus::Drifted {
                    actual_sha256,
                    source_target,
                } => Some(format!(
                    "{} at {} drifted on dx={} dy={} (expected {}, got {})",
                    inspection.slot.name,
                    inspection.slot.location_display(),
                    source_target.dx,
                    source_target.dy,
                    inspection.slot.normalized_sha256,
                    actual_sha256
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

pub fn install_bundled_runtime<R>(
    requested_target: ResolvedTarget,
    reader: &mut R,
) -> Result<RuntimeInstallReport>
where
    R: RuntimeSlotReader + RuntimeSlotWriter + RuntimePageStorer,
{
    install_runtime_with_bundle_dir(
        crate::runtime_bundle::bundled_runtime_dir(),
        requested_target,
        reader,
    )
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
                        let actual_sha256 = normalized_sha256(&read.content);
                        if actual_sha256 == asset.normalized_sha256 {
                            RuntimeSlotStatus::Match {
                                source_target: read.source_target,
                            }
                        } else {
                            RuntimeSlotStatus::Drifted {
                                actual_sha256,
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
    fn verify_fails_when_owned_slot_content_drifted() {
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

        let error =
            verify_bundled_runtime(ResolvedTarget::Explicit(GridTarget::new(0, 0)), &mut reader)
                .unwrap_err();

        assert!(error
            .to_string()
            .contains("lcd-draw at page=0 element=13 event=8 drifted"));
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
        let root = fixture.path();
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

[[owned_slots]]
name = "second"
page = 0
element = 13
event = 8
asset = "second.lua"
install_order = 20
normalized_sha256 = "{}"
runtime_marker = "fixture:second"

[[owned_slots]]
name = "first"
page = 0
element = 13
event = 0
asset = "first.lua"
install_order = 10
normalized_sha256 = "{}"
runtime_marker = "fixture:first"
"#,
                normalized_sha256(&frame_lua(second_content)),
                normalized_sha256(&frame_lua(first_content)),
            ),
        )
        .unwrap();

        let mut accessor = RecordingSlotAccessor {
            persist_writes: true,
            ..Default::default()
        };
        let report = install_runtime_with_bundle_dir(
            root,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut accessor,
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
    }

    #[test]
    fn install_fails_when_page_store_is_rejected() {
        let mut accessor = RecordingSlotAccessor {
            reject_page_store: true,
            ..Default::default()
        };

        let error = install_bundled_runtime(
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut accessor,
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
        let mut accessor = RecordingSlotAccessor {
            persist_writes: true,
            drifted_slot: Some("lcd-draw".to_string()),
            ..Default::default()
        };

        let error = install_bundled_runtime(
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &mut accessor,
        )
        .unwrap_err();

        assert!(error.to_string().contains(
            "post-install bundled runtime 2026-06-17-screen-first.5 is not an exact match"
        ));
        assert!(error
            .to_string()
            .contains("lcd-draw at page=0 element=13 event=8 drifted"));
    }
}
