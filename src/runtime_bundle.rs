use std::collections::HashSet;
use std::error::Error as StdError;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::protocol::{frame_lua, GRID_MAX_LUA_BYTES};

pub const BUNDLED_RUNTIME_VERSION: &str = "2026-06-17-screen-first.8";

const BUNDLED_RUNTIME_ROOT: &str = "assets/runtime";
const MANIFEST_FILE_NAME: &str = "manifest.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeBundleError {
    ReadManifest {
        path: PathBuf,
        message: String,
    },
    ParseManifest {
        path: PathBuf,
        message: String,
    },
    ReadAsset {
        path: PathBuf,
        message: String,
    },
    InvalidManifest {
        message: String,
    },
    AssetHashMismatch {
        asset: String,
        expected: String,
        actual: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBundle {
    root: PathBuf,
    manifest: RuntimeBundleManifest,
    assets: Vec<RuntimeAsset>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RuntimeBundleManifest {
    pub bundle_version: String,
    pub compatibility_reference: String,
    pub runtime_marker: String,
    #[serde(default)]
    pub compatibility_notes: Vec<String>,
    pub owned_slots: Vec<OwnedRuntimeSlot>,
    #[serde(default)]
    pub fields: Vec<RuntimeFieldSpec>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct OwnedRuntimeSlot {
    pub name: String,
    pub page: u8,
    pub element: u8,
    pub event: u8,
    pub asset: String,
    pub install_order: u32,
    pub normalized_sha256: String,
    pub runtime_marker: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RuntimeFieldSpec {
    pub name: String,
    pub layer: String,
    pub value_kind: String,
    pub runtime_key: String,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeAsset {
    pub slot: OwnedRuntimeSlot,
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
            Self::AssetHashMismatch {
                asset,
                expected,
                actual,
            } => write!(
                f,
                "runtime asset hash mismatch for {asset}: expected {expected}, got {actual}"
            ),
        }
    }
}

impl StdError for RuntimeBundleError {}

impl RuntimeBundle {
    pub fn bundled() -> Result<Self> {
        Self::load_from_dir(bundled_runtime_dir())
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
        let manifest: RuntimeBundleManifest =
            toml::from_str(&manifest_text).map_err(|error| RuntimeBundleError::ParseManifest {
                path: manifest_path,
                message: error.to_string(),
            })?;

        validate_manifest(&manifest)?;

        let mut assets = manifest
            .owned_slots
            .iter()
            .cloned()
            .map(|slot| load_runtime_asset(&root, slot))
            .collect::<Result<Vec<_>>>()?;
        assets.sort_by_key(|asset| asset.slot.install_order);

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
}

pub type Result<T> = std::result::Result<T, RuntimeBundleError>;

pub fn bundled_runtime_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(BUNDLED_RUNTIME_ROOT)
        .join(BUNDLED_RUNTIME_VERSION)
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

fn validate_manifest(manifest: &RuntimeBundleManifest) -> Result<()> {
    if manifest.bundle_version.trim().is_empty() {
        return Err(RuntimeBundleError::InvalidManifest {
            message: "bundle_version must not be empty".to_string(),
        });
    }

    if manifest.owned_slots.is_empty() {
        return Err(RuntimeBundleError::InvalidManifest {
            message: "owned_slots must not be empty".to_string(),
        });
    }

    let mut slot_names = HashSet::new();
    let mut slot_locations = HashSet::new();
    for slot in &manifest.owned_slots {
        if !slot_names.insert(slot.name.clone()) {
            return Err(RuntimeBundleError::InvalidManifest {
                message: format!("duplicate owned slot name {}", slot.name),
            });
        }

        if !slot_locations.insert((slot.page, slot.element, slot.event)) {
            return Err(RuntimeBundleError::InvalidManifest {
                message: format!("duplicate owned slot location {}", slot.location_display()),
            });
        }

        if slot.normalized_sha256.len() != 64
            || !slot
                .normalized_sha256
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err(RuntimeBundleError::InvalidManifest {
                message: format!(
                    "owned slot {} has an invalid normalized_sha256 value",
                    slot.name
                ),
            });
        }
    }

    let mut field_names = HashSet::new();
    for field in &manifest.fields {
        if !field_names.insert(field.name.clone()) {
            return Err(RuntimeBundleError::InvalidManifest {
                message: format!("duplicate field inventory entry {}", field.name),
            });
        }
    }

    Ok(())
}

fn load_runtime_asset(root: &Path, slot: OwnedRuntimeSlot) -> Result<RuntimeAsset> {
    let source_path = resolve_asset_path(root, &slot.asset)?;
    let original_content =
        fs::read_to_string(&source_path).map_err(|error| RuntimeBundleError::ReadAsset {
            path: source_path.clone(),
            message: error.to_string(),
        })?;
    let normalized_content = normalize_text_content(&original_content);
    validate_installable_script_length(&slot, &normalized_content)?;
    let stored_content = normalize_text_content(&frame_lua(&normalized_content));
    let actual_hash = sha256_hex(stored_content.as_bytes());

    if actual_hash != slot.normalized_sha256 {
        return Err(RuntimeBundleError::AssetHashMismatch {
            asset: slot.asset.clone(),
            expected: slot.normalized_sha256.clone(),
            actual: actual_hash,
        });
    }

    Ok(RuntimeAsset {
        slot,
        source_path,
        original_content,
        normalized_content,
        stored_content,
        normalized_sha256: actual_hash,
    })
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

fn validate_installable_script_length(slot: &OwnedRuntimeSlot, content: &str) -> Result<()> {
    let framed_len = frame_lua(content).len();
    let max_len = GRID_MAX_LUA_BYTES - 1;

    if framed_len > max_len {
        return Err(RuntimeBundleError::InvalidManifest {
            message: format!(
                "owned slot {} exceeds the Grid CONFIG payload limit after Lua framing: {} bytes (maximum {})",
                slot.name, framed_len, max_len
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

    #[test]
    fn bundled_runtime_manifest_and_assets_load() {
        let bundle = RuntimeBundle::bundled().unwrap();

        assert_eq!(bundle.manifest().bundle_version, BUNDLED_RUNTIME_VERSION);
        assert_eq!(bundle.assets().len(), 2);
        assert_eq!(bundle.assets()[0].slot.name, "lcd-init");
        assert_eq!(bundle.assets()[1].slot.name, "lcd-draw");
        assert!(bundle
            .manifest()
            .fields
            .iter()
            .any(|field| field.name == "fast.action"));
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
    fn reports_hash_mismatches_from_the_manifest() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        let asset_path = root.join("lcd-init.lua");
        fs::write(&asset_path, "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            r#"
bundle_version = "test"
compatibility_reference = "fixture"
runtime_marker = "fixture"

[[owned_slots]]
name = "lcd-init"
page = 0
element = 13
event = 0
asset = "lcd-init.lua"
install_order = 10
normalized_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
runtime_marker = "fixture:lcd-init"
"#,
        )
        .unwrap();

        let error = RuntimeBundle::load_from_dir(root).unwrap_err();

        assert_eq!(
            error.to_string(),
            "runtime asset hash mismatch for lcd-init.lua: expected aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, got 21cc312dc0da113b58ff40217cdafb0505e9602737287e43bfceb64c5997e4df"
        );
    }

    #[test]
    fn rejects_duplicate_field_inventory_entries() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        let hash = normalized_sha256("return 1\n");

        fs::write(root.join("lcd-init.lua"), "return 1\n").unwrap();
        fs::write(
            root.join("manifest.toml"),
            format!(
                r#"
bundle_version = "test"
compatibility_reference = "fixture"
runtime_marker = "fixture"

[[owned_slots]]
name = "lcd-init"
page = 0
element = 13
event = 0
asset = "lcd-init.lua"
install_order = 10
normalized_sha256 = "{hash}"
runtime_marker = "fixture:lcd-init"

[[fields]]
name = "persistent.title"
layer = "persistent"
value_kind = "text"
runtime_key = "persistent.title"
notes = "fixture"

[[fields]]
name = "persistent.title"
layer = "persistent"
value_kind = "text"
runtime_key = "persistent.title"
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
    fn rejects_owned_slot_scripts_that_exceed_the_grid_config_limit() {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        let oversized = "a".repeat(909);
        let hash = normalized_sha256(&frame_lua(&oversized));

        fs::write(root.join("lcd-init.lua"), oversized).unwrap();
        fs::write(
            root.join("manifest.toml"),
            format!(
                r#"
bundle_version = "test"
compatibility_reference = "fixture"
runtime_marker = "fixture"

[[owned_slots]]
name = "lcd-init"
page = 0
element = 13
event = 0
asset = "lcd-init.lua"
install_order = 10
normalized_sha256 = "{hash}"
runtime_marker = "fixture:lcd-init"
"#
            ),
        )
        .unwrap();

        let error = RuntimeBundle::load_from_dir(root).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid runtime manifest: owned slot lcd-init exceeds the Grid CONFIG payload limit after Lua framing: 928 bytes (maximum 908)"
        );
    }
}
