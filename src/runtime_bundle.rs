use std::collections::{BTreeMap, HashSet};
use std::error::Error as StdError;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::protocol::{frame_immediate_lua, frame_lua, GRID_MAX_LUA_BYTES};

pub const BUNDLED_RUNTIME_NAME: &str = "default";

const BUNDLED_RUNTIME_ROOT: &str = "assets/runtimes";
const SYSTEM_RUNTIME_ROOT: &str = "/usr/share/vsn1-cli/runtimes";
const MANIFEST_FILE_NAME: &str = "manifest.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeBundleError {
    ReadManifest { path: PathBuf, message: String },
    ReadBundleRoot { path: PathBuf, message: String },
    ParseManifest { path: PathBuf, message: String },
    ReadAsset { path: PathBuf, message: String },
    InvalidManifest { message: String },
    RuntimeNotFound { name: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeBundle {
    root: PathBuf,
    manifest: RuntimeBundleManifest,
    assets: Vec<RuntimeAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuntimeSource {
    System,
    User,
    Dev,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRoot {
    pub source: RuntimeSource,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredRuntime {
    pub name: String,
    pub source: RuntimeSource,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RuntimeBundleManifest {
    #[serde(default)]
    pub provisioning_backend: RuntimeProvisioningBackend,
    pub layers: Vec<RuntimeLayerSpec>,
    #[serde(default)]
    pub owned_slots: Vec<OwnedRuntimeSlot>,
    #[serde(default)]
    pub owned_files: Vec<OwnedRuntimeFile>,
    #[serde(default)]
    pub fields: Vec<RuntimeFieldSpec>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeProvisioningBackend {
    #[default]
    ConfigSlots,
    ModuleFiles,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RuntimeLayerSpec {
    pub name: String,
    pub priority: u32,
    pub activation: RuntimeLayerActivation,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeLayerActivation {
    Persistent,
    Temporary,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct OwnedRuntimeSlot {
    pub name: String,
    pub page: u8,
    pub element: u8,
    pub event: u8,
    pub asset: String,
    pub install_order: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct OwnedRuntimeFile {
    pub name: String,
    pub path: String,
    pub asset: String,
    pub install_order: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeOwnedAssetLocation {
    Slot(OwnedRuntimeSlot),
    File(OwnedRuntimeFile),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeOwnedAsset {
    pub name: String,
    pub asset: String,
    pub install_order: u32,
    pub location: RuntimeOwnedAssetLocation,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RuntimeFieldSpec {
    pub name: String,
    pub layer: String,
    pub value_kind: String,
    pub runtime_key: String,
    pub clear_value: toml::Value,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeAsset {
    pub owned: RuntimeOwnedAsset,
    pub source_path: PathBuf,
    pub original_content: String,
    pub normalized_content: String,
    pub stored_content: String,
    pub normalized_sha256: String,
}

impl fmt::Display for RuntimeBundleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadManifest { path, message } => {
                write!(
                    f,
                    "failed to read runtime manifest {}: {message}",
                    path.display()
                )
            }
            Self::ReadBundleRoot { path, message } => {
                write!(
                    f,
                    "failed to read runtime bundle directory {}: {message}",
                    path.display()
                )
            }
            Self::ParseManifest { path, message } => {
                write!(
                    f,
                    "failed to parse runtime manifest {}: {message}",
                    path.display()
                )
            }
            Self::ReadAsset { path, message } => {
                write!(
                    f,
                    "failed to read runtime asset {}: {message}",
                    path.display()
                )
            }
            Self::InvalidManifest { message } => write!(f, "invalid runtime manifest: {message}"),
            Self::RuntimeNotFound { name } => write!(f, "runtime `{name}` was not found"),
        }
    }
}

impl StdError for RuntimeBundleError {}

impl RuntimeBundle {
    pub fn bundled() -> Result<Self> {
        Self::load_from_dir(bundled_runtime_dir())
    }

    pub fn load_bundled_family() -> Result<Vec<Self>> {
        Self::load_family_from_dir(bundled_runtime_root_dir())
    }

    pub fn load_family_from_dir(path: impl AsRef<Path>) -> Result<Vec<Self>> {
        let root = path.as_ref().to_path_buf();
        let entries = fs::read_dir(&root).map_err(|error| RuntimeBundleError::ReadBundleRoot {
            path: root.clone(),
            message: error.to_string(),
        })?;
        let mut bundles = Vec::new();

        for entry in entries {
            let entry = entry.map_err(|error| RuntimeBundleError::ReadBundleRoot {
                path: root.clone(),
                message: error.to_string(),
            })?;
            let path = entry.path();
            if !path.is_dir() || !path.join(MANIFEST_FILE_NAME).is_file() {
                continue;
            }

            bundles.push(Self::load_from_dir(path)?);
        }

        bundles.sort_by(|left, right| left.root.cmp(&right.root));
        Ok(bundles)
    }

    pub fn load_from_dir(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref().to_path_buf();
        let manifest_path = root.join(MANIFEST_FILE_NAME);
        let manifest_text = fs::read_to_string(&manifest_path).map_err(|error| {
            RuntimeBundleError::ReadManifest {
                path: manifest_path.clone(),
                message: error.to_string(),
            }
        })?;
        let manifest = parse_runtime_bundle_manifest_text(&manifest_path, &manifest_text)?;

        let mut assets = Vec::new();
        for slot in manifest.owned_slots.iter().cloned() {
            assets.push(load_runtime_asset(
                &root,
                manifest.provisioning_backend,
                RuntimeOwnedAsset {
                    name: slot.name.clone(),
                    asset: slot.asset.clone(),
                    install_order: slot.install_order,
                    location: RuntimeOwnedAssetLocation::Slot(slot),
                },
            )?);
        }
        for file in manifest.owned_files.iter().cloned() {
            assets.push(load_runtime_asset(
                &root,
                manifest.provisioning_backend,
                RuntimeOwnedAsset {
                    name: file.name.clone(),
                    asset: file.asset.clone(),
                    install_order: file.install_order,
                    location: RuntimeOwnedAssetLocation::File(file),
                },
            )?);
        }
        assets.sort_by_key(|asset| asset.owned.install_order);

        Ok(Self {
            root,
            manifest,
            assets,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &RuntimeBundleManifest {
        &self.manifest
    }

    pub fn assets(&self) -> &[RuntimeAsset] {
        &self.assets
    }
}

impl OwnedRuntimeSlot {
    pub fn location_display(&self) -> String {
        format!(
            "page={} element={} event={}",
            self.page, self.element, self.event
        )
    }

    pub fn derived_module_file_path(&self) -> String {
        format!(
            "/{:02x}/{:02x}/{:02x}.cfg",
            self.page, self.element, self.event
        )
    }
}

impl OwnedRuntimeFile {
    pub fn location_display(&self) -> String {
        self.path.clone()
    }
}

impl RuntimeOwnedAsset {
    pub fn location_display(&self) -> String {
        match &self.location {
            RuntimeOwnedAssetLocation::Slot(slot) => slot.location_display(),
            RuntimeOwnedAssetLocation::File(file) => file.location_display(),
        }
    }

    pub fn module_file_path(&self) -> Option<String> {
        match &self.location {
            RuntimeOwnedAssetLocation::Slot(slot) => Some(slot.derived_module_file_path()),
            RuntimeOwnedAssetLocation::File(file) => Some(file.path.clone()),
        }
    }

    pub fn slot_location(&self) -> Option<&OwnedRuntimeSlot> {
        match &self.location {
            RuntimeOwnedAssetLocation::Slot(slot) => Some(slot),
            RuntimeOwnedAssetLocation::File(_) => None,
        }
    }

    pub fn page(&self) -> Option<u8> {
        self.slot_location().map(|slot| slot.page)
    }
}

impl RuntimeSource {
    fn priority(self) -> u8 {
        match self {
            Self::System => 0,
            Self::User => 1,
            Self::Dev => 2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Dev => "dev",
        }
    }
}

pub type Result<T> = std::result::Result<T, RuntimeBundleError>;

pub fn bundled_runtime_dir() -> PathBuf {
    bundled_runtime_root_dir().join(BUNDLED_RUNTIME_NAME)
}

pub fn bundled_runtime_root_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(BUNDLED_RUNTIME_ROOT)
}

pub fn system_runtime_root_dir() -> PathBuf {
    PathBuf::from(SYSTEM_RUNTIME_ROOT)
}

pub fn user_runtime_root_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .map(|home| home.join(".local/share/vsn1-cli/runtimes"))
}

pub fn runtime_roots() -> Vec<RuntimeRoot> {
    let mut roots = Vec::new();

    let system_root = system_runtime_root_dir();
    if system_root.is_dir() {
        roots.push(RuntimeRoot {
            source: RuntimeSource::System,
            path: system_root,
        });
    }

    if let Some(user_root) = user_runtime_root_dir().filter(|root| root.is_dir()) {
        roots.push(RuntimeRoot {
            source: RuntimeSource::User,
            path: user_root,
        });
    }

    let dev_root = bundled_runtime_root_dir();
    if dev_root.is_dir() {
        roots.push(RuntimeRoot {
            source: RuntimeSource::Dev,
            path: dev_root,
        });
    }

    roots
}

pub fn discover_runtimes() -> Result<Vec<DiscoveredRuntime>> {
    discover_runtimes_in_roots(&runtime_roots())
}

pub fn resolve_runtime(name: &str) -> Result<DiscoveredRuntime> {
    discover_runtimes()?
        .into_iter()
        .find(|runtime| runtime.name == name)
        .ok_or_else(|| RuntimeBundleError::RuntimeNotFound {
            name: name.to_string(),
        })
}

fn discover_runtimes_in_roots(roots: &[RuntimeRoot]) -> Result<Vec<DiscoveredRuntime>> {
    let mut runtimes = BTreeMap::<String, DiscoveredRuntime>::new();

    for root in roots {
        if !root.path.exists() {
            continue;
        }

        let entries =
            fs::read_dir(&root.path).map_err(|error| RuntimeBundleError::ReadBundleRoot {
                path: root.path.clone(),
                message: error.to_string(),
            })?;

        for entry in entries {
            let entry = entry.map_err(|error| RuntimeBundleError::ReadBundleRoot {
                path: root.path.clone(),
                message: error.to_string(),
            })?;
            let path = entry.path();
            if !path.is_dir() || !path.join(MANIFEST_FILE_NAME).is_file() {
                continue;
            }

            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            let candidate = DiscoveredRuntime {
                name: name.to_string(),
                source: root.source,
                path: path.clone(),
            };

            match runtimes.get(name) {
                Some(existing) if existing.source.priority() >= candidate.source.priority() => {}
                _ => {
                    runtimes.insert(name.to_string(), candidate);
                }
            }
        }
    }

    Ok(runtimes.into_values().collect())
}

pub fn normalize_text_content(content: &str) -> String {
    let normalized_newlines = content.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized_newlines.trim_end_matches('\n');

    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

pub fn normalized_sha256(content: &str) -> String {
    sha256_hex(normalize_text_content(content).as_bytes())
}

pub(crate) fn parse_runtime_bundle_manifest_text(
    manifest_path: impl AsRef<Path>,
    manifest_text: &str,
) -> Result<RuntimeBundleManifest> {
    let manifest_path = manifest_path.as_ref().to_path_buf();
    let manifest: RuntimeBundleManifest =
        toml::from_str(manifest_text).map_err(|error| RuntimeBundleError::ParseManifest {
            path: manifest_path,
            message: error.to_string(),
        })?;

    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &RuntimeBundleManifest) -> Result<()> {
    validate_layers(manifest)?;

    if manifest.owned_slots.is_empty() && manifest.owned_files.is_empty() {
        return Err(RuntimeBundleError::InvalidManifest {
            message: "owned_slots and owned_files must not both be empty".to_string(),
        });
    }

    let mut asset_names = HashSet::new();
    let mut slot_locations = HashSet::new();
    for slot in &manifest.owned_slots {
        if !asset_names.insert(slot.name.clone()) {
            return Err(RuntimeBundleError::InvalidManifest {
                message: format!("duplicate owned slot name {}", slot.name),
            });
        }

        if !slot_locations.insert((slot.page, slot.element, slot.event)) {
            return Err(RuntimeBundleError::InvalidManifest {
                message: format!("duplicate owned slot location {}", slot.location_display()),
            });
        }

        if manifest.provisioning_backend == RuntimeProvisioningBackend::ModuleFiles
            && !Path::new(&slot.asset)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("lua"))
        {
            return Err(RuntimeBundleError::InvalidManifest {
                message: format!(
                    "module-files provisioning currently supports only runtime-owned .lua event files; slot {} uses asset {}",
                    slot.name, slot.asset
                ),
            });
        }
    }

    if manifest.provisioning_backend == RuntimeProvisioningBackend::ConfigSlots
        && !manifest.owned_files.is_empty()
    {
        let file = &manifest.owned_files[0];
        return Err(RuntimeBundleError::InvalidManifest {
            message: format!(
                "owned_files is only supported for module-files provisioning; file {} targets {}",
                file.name, file.path
            ),
        });
    }

    let mut file_paths = HashSet::new();
    for file in &manifest.owned_files {
        if !asset_names.insert(file.name.clone()) {
            return Err(RuntimeBundleError::InvalidManifest {
                message: format!("duplicate owned file name {}", file.name),
            });
        }

        validate_owned_runtime_file(file)?;

        if !file_paths.insert(file.path.clone()) {
            return Err(RuntimeBundleError::InvalidManifest {
                message: format!("duplicate owned file path {}", file.path),
            });
        }
    }

    let declared_layers = manifest
        .layers
        .iter()
        .map(|layer| layer.name.as_str())
        .collect::<HashSet<_>>();
    let mut field_names = HashSet::new();
    for field in &manifest.fields {
        if !field_names.insert(field.name.clone()) {
            return Err(RuntimeBundleError::InvalidManifest {
                message: format!("duplicate field inventory entry {}", field.name),
            });
        }

        if !declared_layers.contains(field.layer.as_str()) {
            return Err(RuntimeBundleError::InvalidManifest {
                message: format!(
                    "field {} references undeclared layer {}",
                    field.name, field.layer
                ),
            });
        }
    }

    Ok(())
}

fn validate_owned_runtime_file(file: &OwnedRuntimeFile) -> Result<()> {
    let path = &file.path;
    let device_path = Path::new(path);

    if path.trim().is_empty()
        || !device_path.is_absolute()
        || device_path == Path::new("/")
        || device_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::CurDir | Component::Prefix(_)
            )
        })
    {
        return Err(RuntimeBundleError::InvalidManifest {
            message: format!(
                "owned file path for {} must be an absolute module file path without traversal; got {}",
                file.name, path
            ),
        });
    }

    if !device_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lua"))
    {
        return Err(RuntimeBundleError::InvalidManifest {
            message: format!(
                "owned file path for {} must point to a .lua helper file; got {}",
                file.name, path
            ),
        });
    }

    Ok(())
}

fn validate_layers(manifest: &RuntimeBundleManifest) -> Result<()> {
    let mut layer_names = HashSet::new();
    let mut priorities = HashSet::new();
    let mut persistent_layer_count = 0usize;

    for layer in &manifest.layers {
        if layer.name.trim().is_empty() {
            return Err(RuntimeBundleError::InvalidManifest {
                message: "layer name must not be empty".to_string(),
            });
        }

        if !layer_names.insert(layer.name.clone()) {
            return Err(RuntimeBundleError::InvalidManifest {
                message: format!("duplicate layer name {}", layer.name),
            });
        }

        if !priorities.insert(layer.priority) {
            return Err(RuntimeBundleError::InvalidManifest {
                message: format!("duplicate layer priority {}", layer.priority),
            });
        }

        match layer.activation {
            RuntimeLayerActivation::Persistent => {
                persistent_layer_count += 1;
                if layer.timeout_ms.is_some() {
                    return Err(RuntimeBundleError::InvalidManifest {
                        message: format!(
                            "persistent layer {} must not define timeout_ms",
                            layer.name
                        ),
                    });
                }
            }
            RuntimeLayerActivation::Temporary => {
                if layer.timeout_ms.is_none() {
                    return Err(RuntimeBundleError::InvalidManifest {
                        message: format!("temporary layer {} must define timeout_ms", layer.name),
                    });
                }
            }
        }
    }

    if persistent_layer_count == 0 {
        Err(RuntimeBundleError::InvalidManifest {
            message: "at least one persistent layer must be declared".to_string(),
        })
    } else {
        Ok(())
    }
}

fn load_runtime_asset(
    root: &Path,
    provisioning_backend: RuntimeProvisioningBackend,
    owned: RuntimeOwnedAsset,
) -> Result<RuntimeAsset> {
    let source_path = resolve_asset_path(root, &owned.asset)?;
    let original_content =
        fs::read_to_string(&source_path).map_err(|error| RuntimeBundleError::ReadAsset {
            path: source_path.clone(),
            message: error.to_string(),
        })?;
    let normalized_content =
        normalize_runtime_script_content(provisioning_backend, &owned, &original_content);
    if provisioning_backend == RuntimeProvisioningBackend::ConfigSlots {
        validate_installable_script_length(&owned, &normalized_content)?;
    }
    let stored_content =
        stored_runtime_script_content(provisioning_backend, &owned, &normalized_content);
    let actual_hash = sha256_hex(stored_content.as_bytes());

    Ok(RuntimeAsset {
        owned,
        source_path,
        original_content,
        normalized_content,
        stored_content,
        normalized_sha256: actual_hash,
    })
}

fn normalize_runtime_script_content(
    provisioning_backend: RuntimeProvisioningBackend,
    owned: &RuntimeOwnedAsset,
    content: &str,
) -> String {
    let normalized = normalize_text_content(content);

    match (provisioning_backend, &owned.location) {
        (_, RuntimeOwnedAssetLocation::File(_)) => normalized,
        (_, RuntimeOwnedAssetLocation::Slot(_)) => {
            if normalized.is_empty() {
                String::new()
            } else {
                frame_immediate_lua(&normalized)
            }
        }
    }
}

fn stored_runtime_script_content(
    provisioning_backend: RuntimeProvisioningBackend,
    owned: &RuntimeOwnedAsset,
    normalized_content: &str,
) -> String {
    match provisioning_backend {
        RuntimeProvisioningBackend::ConfigSlots => {
            normalize_text_content(&frame_immediate_lua(normalized_content))
        }
        RuntimeProvisioningBackend::ModuleFiles => match &owned.location {
            RuntimeOwnedAssetLocation::Slot(_) => {
                if normalized_content.is_empty() {
                    String::new()
                } else {
                    frame_lua(&compact_module_file_lua_body(normalized_content))
                }
            }
            RuntimeOwnedAssetLocation::File(_) => normalized_content.to_string(),
        },
    }
}

fn compact_module_file_lua_body(content: &str) -> String {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn resolve_asset_path(root: &Path, asset: &str) -> Result<PathBuf> {
    let asset_path = Path::new(asset);
    if asset_path.is_absolute()
        || asset_path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return Err(RuntimeBundleError::InvalidManifest {
            message: format!("asset path {asset} must stay within the bundle directory"),
        });
    }

    Ok(root.join(asset_path))
}

fn validate_installable_script_length(owned: &RuntimeOwnedAsset, content: &str) -> Result<()> {
    let framed_len = frame_immediate_lua(content).len();
    let max_len = GRID_MAX_LUA_BYTES - 1;

    if framed_len > max_len {
        return Err(RuntimeBundleError::InvalidManifest {
            message: format!(
                "owned slot {} exceeds the Grid CONFIG payload limit after Lua framing: {} bytes (maximum {})",
                owned.name, framed_len, max_len
            ),
        });
    }

    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);

    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use tempfile::tempdir;

    fn write_fixture_runtime(root: &Path, name: &str) {
        let runtime_root = root.join(name);
        let content = "return 1\n";
        fs::create_dir_all(&runtime_root).unwrap();
        fs::write(runtime_root.join("lcd-init.lua"), content).unwrap();
        fs::write(
            runtime_root.join("manifest.toml"),
            format!(
                r#"
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
"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn bundled_runtime_manifest_and_assets_load() {
        let bundle = RuntimeBundle::bundled().unwrap();

        assert_eq!(bundle.root(), bundled_runtime_dir().as_path());
        assert_eq!(
            bundle.manifest().provisioning_backend,
            RuntimeProvisioningBackend::ConfigSlots
        );
        assert_eq!(bundle.assets().len(), 2);
        assert_eq!(bundle.assets()[0].owned.name, "lcd-init");
        assert_eq!(bundle.assets()[1].owned.name, "lcd-draw");
        assert!(bundle
            .manifest()
            .fields
            .iter()
            .any(|field| field.name == "fast.action"));
        assert_eq!(
            bundle
                .manifest()
                .layers
                .iter()
                .map(|layer| layer.name.as_str())
                .collect::<Vec<_>>(),
            vec!["persistent", "slow", "fast"]
        );
    }

    #[test]
    fn bundled_runtime_uses_generic_layer_helpers_within_config_limit() {
        let bundle = RuntimeBundle::bundled().unwrap();
        let init = bundle
            .assets()
            .iter()
            .find(|asset| asset.owned.name == "lcd-init")
            .unwrap();
        let draw = bundle
            .assets()
            .iter()
            .find(|asset| asset.owned.name == "lcd-draw")
            .unwrap();

        assert!(init.normalized_content.contains("function set_field("));
        assert!(init.normalized_content.contains("function activate_layer("));
        assert!(!init.normalized_content.contains("update_param="));
        assert!(draw.normalized_content.contains("local l,p,e=z.b,z.l,z.u"));
        assert!(draw.normalized_content.contains("for i=1,#z.o do"));

        for asset in bundle.assets() {
            assert!(frame_immediate_lua(&asset.normalized_content).len() <= GRID_MAX_LUA_BYTES - 1);
        }
    }

    #[test]
    fn bundled_runtime_family_is_sorted_and_includes_current_bundle() {
        let bundles = RuntimeBundle::load_bundled_family().unwrap();

        assert!(!bundles.is_empty());
        assert!(bundles
            .iter()
            .any(|bundle| bundle.root() == bundled_runtime_dir()));

        let names = bundles
            .iter()
            .map(|bundle| {
                bundle
                    .root()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let mut sorted_names = names.clone();
        sorted_names.sort();

        assert_eq!(names, sorted_names);
    }

    #[test]
    fn media_runtime_manifest_and_assets_load_with_module_files_backend() {
        let bundle =
            RuntimeBundle::load_from_dir(bundled_runtime_root_dir().join("media")).unwrap();

        assert_eq!(
            bundle.manifest().provisioning_backend,
            RuntimeProvisioningBackend::ModuleFiles
        );
        assert_eq!(bundle.assets().len(), 3);
        assert_eq!(
            bundle
                .assets()
                .iter()
                .map(|asset| asset.owned.module_file_path().unwrap())
                .collect::<Vec<_>>(),
            vec!["/vsn1_media_runtime.lua", "/00/0d/00.cfg", "/00/0d/08.cfg",]
        );
        assert_eq!(
            bundle
                .manifest()
                .layers
                .iter()
                .map(|layer| (layer.name.as_str(), layer.activation, layer.timeout_ms))
                .collect::<Vec<_>>(),
            vec![
                ("base", RuntimeLayerActivation::Persistent, None),
                ("player", RuntimeLayerActivation::Temporary, Some(5000)),
                (
                    "playback_status",
                    RuntimeLayerActivation::Temporary,
                    Some(2000)
                ),
            ]
        );
        assert!(bundle
            .manifest()
            .fields
            .iter()
            .any(|field| field.name == "base.duration"));
        assert!(bundle
            .manifest()
            .fields
            .iter()
            .any(|field| field.name == "player.name"));
        assert!(bundle
            .manifest()
            .fields
            .iter()
            .any(|field| field.name == "playback_status.status"));

        for asset in bundle.assets() {
            match asset.owned.name.as_str() {
                "lcd-init" | "lcd-draw" => {
                    assert!(asset.stored_content.starts_with("<?lua --[[@cb]]"));
                    assert!(!asset.stored_content.contains('\n'));
                }
                "media-runtime-module" => {
                    assert!(!asset.stored_content.starts_with("<?lua "));
                    assert!(asset.stored_content.contains('\n'));
                }
                other => panic!("unexpected media asset {other}"),
            }
        }

        let init = bundle
            .assets()
            .iter()
            .find(|asset| asset.owned.name == "lcd-init")
            .unwrap();
        assert!(init.normalized_content.contains("runtime.init()"));
        assert!(init
            .normalized_content
            .contains("set_field=runtime.set_field"));
        assert!(init
            .normalized_content
            .contains("activate_layer=runtime.activate_layer"));

        let runtime = bundle
            .assets()
            .iter()
            .find(|asset| asset.owned.name == "media-runtime-module")
            .unwrap();
        assert!(runtime
            .normalized_content
            .contains("function Module.set_field("));
        assert!(runtime
            .normalized_content
            .contains("function Module.activate_layer("));
        assert!(runtime
            .normalized_content
            .contains("function Module.draw(self)"));
    }

    #[test]
    fn rejects_owned_files_for_config_slot_provisioning() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        fs::write(root.join("lcd-init.lua"), "return 1\n").unwrap();
        fs::write(root.join("helper.lua"), "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            r#"
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

[[owned_files]]
name = "helper"
path = "/helper.lua"
asset = "helper.lua"
install_order = 20
"#,
        )
        .unwrap();

        let error = RuntimeBundle::load_from_dir(root).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid runtime manifest: owned_files is only supported for module-files provisioning; file helper targets /helper.lua"
        );
    }

    #[test]
    fn discovers_runtime_names_from_all_roots_with_precedence() {
        let fixture = tempdir().unwrap();
        let system_root = fixture.path().join("system");
        let user_root = fixture.path().join("user");
        let dev_root = fixture.path().join("dev");

        write_fixture_runtime(&system_root, "default");
        write_fixture_runtime(&user_root, "default");
        write_fixture_runtime(&dev_root, "default");
        write_fixture_runtime(&system_root, "legacy");
        write_fixture_runtime(&user_root, "local");

        let discovered = discover_runtimes_in_roots(&[
            RuntimeRoot {
                source: RuntimeSource::System,
                path: system_root.clone(),
            },
            RuntimeRoot {
                source: RuntimeSource::User,
                path: user_root.clone(),
            },
            RuntimeRoot {
                source: RuntimeSource::Dev,
                path: dev_root.clone(),
            },
        ])
        .unwrap();

        assert_eq!(
            discovered
                .iter()
                .map(|runtime| (runtime.name.as_str(), runtime.source))
                .collect::<Vec<_>>(),
            vec![
                ("default", RuntimeSource::Dev),
                ("legacy", RuntimeSource::System),
                ("local", RuntimeSource::User),
            ]
        );
        assert_eq!(
            discovered
                .iter()
                .find(|runtime| runtime.name == "default")
                .unwrap()
                .path,
            dev_root.join("default")
        );
    }

    #[test]
    fn discovery_ignores_non_runtime_directories() {
        let fixture = tempdir().unwrap();
        let root = fixture.path().join("dev");

        fs::create_dir_all(root.join("not-a-runtime")).unwrap();
        write_fixture_runtime(&root, "default");

        let discovered = discover_runtimes_in_roots(&[RuntimeRoot {
            source: RuntimeSource::Dev,
            path: root,
        }])
        .unwrap();

        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].name, "default");
    }

    #[test]
    fn resolve_runtime_reports_missing_name() {
        let error = resolve_runtime("definitely-missing").unwrap_err();

        assert_eq!(
            error.to_string(),
            "runtime `definitely-missing` was not found"
        );
    }

    #[test]
    fn normalizes_line_endings_and_trailing_newlines_before_hashing() {
        let normalized = normalize_text_content("line one\r\nline two\r\n\r\n");

        assert_eq!(normalized, "line one\nline two\n");
        assert_eq!(
            normalized_sha256("line one\nline two\n"),
            normalized_sha256("line one\rline two\r\n\n")
        );
    }

    #[test]
    fn ignores_removed_hash_fields_in_the_manifest() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        let asset_path = root.join("lcd-init.lua");
        fs::write(&asset_path, "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            r#"
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
normalized_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
"#,
        )
        .unwrap();

        let bundle = RuntimeBundle::load_from_dir(root).unwrap();

        assert_eq!(bundle.assets().len(), 1);
        assert_eq!(
            bundle.manifest().provisioning_backend,
            RuntimeProvisioningBackend::ConfigSlots
        );
    }

    #[test]
    fn file_manager_proof_runtime_loads_with_module_files_backend() {
        let bundle = RuntimeBundle::load_from_dir(
            bundled_runtime_root_dir().join("default-file-manager-poc"),
        )
        .unwrap();

        assert_eq!(
            bundle.manifest().provisioning_backend,
            RuntimeProvisioningBackend::ModuleFiles
        );
        assert_eq!(
            bundle
                .assets()
                .iter()
                .map(|asset| asset.owned.module_file_path().unwrap())
                .collect::<Vec<_>>(),
            vec!["/00/0d/00.cfg", "/00/0d/08.cfg"]
        );
        assert!(bundle
            .manifest()
            .fields
            .iter()
            .any(|field| field.name == "fast.action"));
        assert!(bundle
            .assets()
            .iter()
            .all(|asset| asset.stored_content.starts_with("<?lua --[[@cb]]")));
        assert!(bundle
            .assets()
            .iter()
            .all(|asset| !asset.stored_content.contains('\n')));
    }

    #[test]
    fn file_manager_proof_runtime_matches_default_runtime_assets_and_metadata() {
        let default_bundle = RuntimeBundle::bundled().unwrap();
        let file_manager_bundle = RuntimeBundle::load_from_dir(
            bundled_runtime_root_dir().join("default-file-manager-poc"),
        )
        .unwrap();

        assert_eq!(
            file_manager_bundle.manifest().layers,
            default_bundle.manifest().layers
        );
        assert_eq!(
            file_manager_bundle.manifest().owned_slots,
            default_bundle.manifest().owned_slots
        );
        assert_eq!(
            file_manager_bundle.manifest().fields,
            default_bundle.manifest().fields
        );
        assert_eq!(
            file_manager_bundle
                .assets()
                .iter()
                .map(|asset| (asset.owned.name.as_str(), asset.normalized_content.as_str()))
                .collect::<Vec<_>>(),
            default_bundle
                .assets()
                .iter()
                .map(|asset| (asset.owned.name.as_str(), asset.normalized_content.as_str()))
                .collect::<Vec<_>>()
        );
        assert!(default_bundle
            .assets()
            .iter()
            .all(|asset| !asset.stored_content.starts_with("<?lua ")));
        assert!(file_manager_bundle
            .assets()
            .iter()
            .all(|asset| asset.stored_content.starts_with("<?lua --[[@cb]]")));
        assert!(file_manager_bundle
            .assets()
            .iter()
            .all(|asset| !asset.stored_content.contains('\n')));
    }

    #[test]
    fn rejects_duplicate_field_inventory_entries() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        fs::write(root.join("lcd-init.lua"), "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            format!(
                r#"
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

[[fields]]
name = "persistent.title"
layer = "persistent"
value_kind = "text"
runtime_key = "persistent.title"
clear_value = ""
notes = "fixture"

[[fields]]
name = "persistent.title"
layer = "persistent"
value_kind = "text"
runtime_key = "persistent.title"
clear_value = ""
notes = "fixture"
"#
            ),
        )
        .unwrap();

        let error = RuntimeBundle::load_from_dir(root).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid runtime manifest: duplicate field inventory entry persistent.title"
        );
    }

    #[test]
    fn rejects_unknown_provisioning_backend_names() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        fs::write(root.join("lcd-init.lua"), "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            r#"
provisioning_backend = "definitely-not-valid"

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
"#,
        )
        .unwrap();

        let error = RuntimeBundle::load_from_dir(root).unwrap_err();

        assert!(error
            .to_string()
            .contains("unknown variant `definitely-not-valid`"));
    }

    #[test]
    fn rejects_non_lua_assets_for_module_files_backend() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        fs::write(root.join("lcd-init.txt"), "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            r#"
provisioning_backend = "module-files"

[[layers]]
name = "persistent"
priority = 0
activation = "persistent"

[[owned_slots]]
name = "lcd-init"
page = 0
element = 13
event = 0
asset = "lcd-init.txt"
install_order = 10
"#,
        )
        .unwrap();

        let error = RuntimeBundle::load_from_dir(root).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid runtime manifest: module-files provisioning currently supports only runtime-owned .lua event files; slot lcd-init uses asset lcd-init.txt"
        );
    }

    #[test]
    fn rejects_duplicate_layer_names() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        fs::write(root.join("lcd-init.lua"), "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            r#"
[[layers]]
name = "persistent"
priority = 0
activation = "persistent"

[[layers]]
name = "persistent"
priority = 10
activation = "temporary"
timeout_ms = 1000

[[owned_slots]]
name = "lcd-init"
page = 0
element = 13
event = 0
asset = "lcd-init.lua"
install_order = 10
"#,
        )
        .unwrap();

        let error = RuntimeBundle::load_from_dir(root).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid runtime manifest: duplicate layer name persistent"
        );
    }

    #[test]
    fn rejects_duplicate_layer_priorities() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        fs::write(root.join("lcd-init.lua"), "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            r#"
[[layers]]
name = "persistent"
priority = 0
activation = "persistent"

[[layers]]
name = "slow"
priority = 0
activation = "temporary"
timeout_ms = 5000

[[owned_slots]]
name = "lcd-init"
page = 0
element = 13
event = 0
asset = "lcd-init.lua"
install_order = 10
"#,
        )
        .unwrap();

        let error = RuntimeBundle::load_from_dir(root).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid runtime manifest: duplicate layer priority 0"
        );
    }

    #[test]
    fn rejects_missing_persistent_layer() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        fs::write(root.join("lcd-init.lua"), "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            r#"
[[layers]]
name = "slow"
priority = 10
activation = "temporary"
timeout_ms = 5000

[[owned_slots]]
name = "lcd-init"
page = 0
element = 13
event = 0
asset = "lcd-init.lua"
install_order = 10
"#,
        )
        .unwrap();

        let error = RuntimeBundle::load_from_dir(root).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid runtime manifest: at least one persistent layer must be declared"
        );
    }

    #[test]
    fn allows_multiple_persistent_layers() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        fs::write(root.join("lcd-init.lua"), "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            r#"
[[layers]]
name = "persistent"
priority = 0
activation = "persistent"

[[layers]]
name = "base"
priority = 1
activation = "persistent"

[[owned_slots]]
name = "lcd-init"
page = 0
element = 13
event = 0
asset = "lcd-init.lua"
install_order = 10
"#,
        )
        .unwrap();

        let bundle = RuntimeBundle::load_from_dir(root).unwrap();

        assert_eq!(bundle.manifest().layers.len(), 2);
    }

    #[test]
    fn rejects_temporary_layers_without_timeout() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        fs::write(root.join("lcd-init.lua"), "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            r#"
[[layers]]
name = "persistent"
priority = 0
activation = "persistent"

[[layers]]
name = "slow"
priority = 10
activation = "temporary"

[[owned_slots]]
name = "lcd-init"
page = 0
element = 13
event = 0
asset = "lcd-init.lua"
install_order = 10
"#,
        )
        .unwrap();

        let error = RuntimeBundle::load_from_dir(root).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid runtime manifest: temporary layer slow must define timeout_ms"
        );
    }

    #[test]
    fn rejects_persistent_layers_with_timeout() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        fs::write(root.join("lcd-init.lua"), "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            r#"
[[layers]]
name = "persistent"
priority = 0
activation = "persistent"
timeout_ms = 1000

[[owned_slots]]
name = "lcd-init"
page = 0
element = 13
event = 0
asset = "lcd-init.lua"
install_order = 10
"#,
        )
        .unwrap();

        let error = RuntimeBundle::load_from_dir(root).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid runtime manifest: persistent layer persistent must not define timeout_ms"
        );
    }

    #[test]
    fn rejects_fields_that_reference_undeclared_layers() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        fs::write(root.join("lcd-init.lua"), "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            r#"
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

[[fields]]
name = "slow.message"
layer = "slow"
value_kind = "text"
runtime_key = "m"
clear_value = ""
notes = "fixture"
"#,
        )
        .unwrap();

        let error = RuntimeBundle::load_from_dir(root).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid runtime manifest: field slow.message references undeclared layer slow"
        );
    }

    #[test]
    fn rejects_owned_slot_scripts_that_exceed_the_grid_config_limit() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        let oversized = "a".repeat(909);
        fs::write(root.join("lcd-init.lua"), oversized).unwrap();
        fs::write(
            root.join("manifest.toml"),
            format!(
                r#"
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
"#
            ),
        )
        .unwrap();

        let error = RuntimeBundle::load_from_dir(root).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid runtime manifest: owned slot lcd-init exceeds the Grid CONFIG payload limit after Lua framing: 919 bytes (maximum 908)"
        );
    }

    #[test]
    fn module_files_backend_allows_scripts_that_exceed_the_grid_config_limit() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        let oversized = format!("return '{}\n'", "a".repeat(1200));
        fs::write(root.join("helper.lua"), &oversized).unwrap();
        fs::write(
            root.join("manifest.toml"),
            r#"
provisioning_backend = "module-files"

[[layers]]
name = "persistent"
priority = 0
activation = "persistent"

[[owned_files]]
name = "helper"
path = "/helper.lua"
asset = "helper.lua"
install_order = 10
"#,
        )
        .unwrap();

        let bundle = RuntimeBundle::load_from_dir(root).unwrap();

        assert_eq!(bundle.assets().len(), 1);
        assert_eq!(
            bundle.assets()[0].owned.module_file_path().unwrap(),
            "/helper.lua"
        );
        assert_eq!(
            bundle.assets()[0].stored_content,
            normalize_text_content(&oversized)
        );
    }
}
