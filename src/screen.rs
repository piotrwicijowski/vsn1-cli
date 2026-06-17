use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt;

use crate::runtime_bundle::{RuntimeBundle, RuntimeBundleError, RuntimeFieldSpec};

const TEXT_LIST_ITEM_COUNT: usize = 8;
const DEFAULT_INFO_LABEL: &str = "---";

pub type Result<T> = std::result::Result<T, ScreenError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenError {
    Bundle(RuntimeBundleError),
    InvalidRuntimeFieldSpec {
        field: String,
        message: String,
    },
    InvalidAssignmentSyntax {
        input: String,
    },
    UnknownField {
        name: String,
        supported: Vec<String>,
    },
    InvalidValue {
        field: String,
        expected: &'static str,
        actual: String,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ScreenLayer {
    Persistent,
    Slow,
    Fast,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ScreenValueKind {
    Text,
    Int,
    Bool,
    TextList,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenValue {
    Text(String),
    Int(i32),
    Bool(bool),
    TextList(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenFieldSpec {
    public_name: String,
    layer: ScreenLayer,
    value_kind: ScreenValueKind,
    runtime_key: String,
    clear_value: ScreenValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenAssignment {
    field: ScreenFieldSpec,
    value: ScreenValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenFieldRegistry {
    fields: Vec<ScreenFieldSpec>,
    fields_by_name: HashMap<String, usize>,
}

impl fmt::Display for ScreenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bundle(error) => error.fmt(f),
            Self::InvalidRuntimeFieldSpec { field, message } => {
                write!(f, "invalid bundled screen field `{field}`: {message}")
            }
            Self::InvalidAssignmentSyntax { input } => {
                write!(f, "screen assignment `{input}` must use FIELD=VALUE syntax")
            }
            Self::UnknownField { name, supported } => write!(
                f,
                "unknown screen field `{name}` (supported fields: {})",
                supported.join(", ")
            ),
            Self::InvalidValue {
                field,
                expected,
                actual,
            } => write!(
                f,
                "invalid value for screen field `{field}`: expected {expected}, got `{actual}`"
            ),
        }
    }
}

impl StdError for ScreenError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Bundle(error) => Some(error),
            Self::InvalidRuntimeFieldSpec { .. }
            | Self::InvalidAssignmentSyntax { .. }
            | Self::UnknownField { .. }
            | Self::InvalidValue { .. } => None,
        }
    }
}

impl From<RuntimeBundleError> for ScreenError {
    fn from(value: RuntimeBundleError) -> Self {
        Self::Bundle(value)
    }
}

impl ScreenLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Persistent => "persistent",
            Self::Slow => "slow",
            Self::Fast => "fast",
        }
    }

    fn parse(raw: &str, field_name: &str) -> Result<Self> {
        match raw {
            "persistent" => Ok(Self::Persistent),
            "slow" => Ok(Self::Slow),
            "fast" => Ok(Self::Fast),
            _ => Err(ScreenError::InvalidRuntimeFieldSpec {
                field: field_name.to_string(),
                message: format!("unsupported layer `{raw}`"),
            }),
        }
    }
}

impl ScreenValueKind {
    pub fn expected_description(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Int => "an integer",
            Self::Bool => "a boolean (`true` or `false`)",
            Self::TextList => "8 pipe-separated text items",
        }
    }

    fn parse(raw: &str, field_name: &str) -> Result<Self> {
        match raw {
            "text" => Ok(Self::Text),
            "int" => Ok(Self::Int),
            "bool" => Ok(Self::Bool),
            "text_list" => Ok(Self::TextList),
            _ => Err(ScreenError::InvalidRuntimeFieldSpec {
                field: field_name.to_string(),
                message: format!("unsupported value kind `{raw}`"),
            }),
        }
    }
}

impl ScreenValue {
    pub fn kind(&self) -> ScreenValueKind {
        match self {
            Self::Text(_) => ScreenValueKind::Text,
            Self::Int(_) => ScreenValueKind::Int,
            Self::Bool(_) => ScreenValueKind::Bool,
            Self::TextList(_) => ScreenValueKind::TextList,
        }
    }
}

impl ScreenFieldSpec {
    pub fn public_name(&self) -> &str {
        &self.public_name
    }

    pub fn layer(&self) -> ScreenLayer {
        self.layer
    }

    pub fn value_kind(&self) -> ScreenValueKind {
        self.value_kind
    }

    pub fn runtime_key(&self) -> &str {
        &self.runtime_key
    }

    pub fn clear_value(&self) -> &ScreenValue {
        &self.clear_value
    }
}

impl ScreenAssignment {
    pub fn field(&self) -> &ScreenFieldSpec {
        &self.field
    }

    pub fn value(&self) -> &ScreenValue {
        &self.value
    }
}

impl ScreenFieldRegistry {
    pub fn bundled() -> Result<Self> {
        let bundle = RuntimeBundle::bundled()?;
        Self::from_bundle(&bundle)
    }

    pub fn from_bundle(bundle: &RuntimeBundle) -> Result<Self> {
        let mut fields = Vec::with_capacity(bundle.manifest().fields.len());
        let mut fields_by_name = HashMap::with_capacity(bundle.manifest().fields.len());

        for runtime_field in &bundle.manifest().fields {
            let field = build_field_spec(runtime_field)?;
            let field_name = field.public_name.clone();
            let field_index = fields.len();

            if fields_by_name
                .insert(field_name.clone(), field_index)
                .is_some()
            {
                return Err(ScreenError::InvalidRuntimeFieldSpec {
                    field: field_name,
                    message: "duplicate screen field name after host registry conversion"
                        .to_string(),
                });
            }

            fields.push(field);
        }

        Ok(Self {
            fields,
            fields_by_name,
        })
    }

    pub fn fields(&self) -> &[ScreenFieldSpec] {
        &self.fields
    }

    pub fn field(&self, name: &str) -> Result<&ScreenFieldSpec> {
        self.fields_by_name
            .get(name)
            .map(|index| &self.fields[*index])
            .ok_or_else(|| ScreenError::UnknownField {
                name: name.to_string(),
                supported: self
                    .fields
                    .iter()
                    .map(|field| field.public_name.clone())
                    .collect(),
            })
    }

    pub fn fields_for_layer(&self, layer: ScreenLayer) -> Vec<&ScreenFieldSpec> {
        self.fields
            .iter()
            .filter(|field| field.layer == layer)
            .collect()
    }

    pub fn parse_assignment(&self, input: &str) -> Result<ScreenAssignment> {
        let (field_name, raw_value) =
            input
                .split_once('=')
                .ok_or_else(|| ScreenError::InvalidAssignmentSyntax {
                    input: input.to_string(),
                })?;

        if field_name.is_empty() {
            return Err(ScreenError::InvalidAssignmentSyntax {
                input: input.to_string(),
            });
        }

        let field = self.field(field_name)?.clone();
        let value = parse_value(&field, raw_value)?;

        Ok(ScreenAssignment { field, value })
    }

    pub fn parse_assignments<S>(
        &self,
        assignments: impl IntoIterator<Item = S>,
    ) -> Result<Vec<ScreenAssignment>>
    where
        S: AsRef<str>,
    {
        assignments
            .into_iter()
            .map(|assignment| self.parse_assignment(assignment.as_ref()))
            .collect()
    }

    pub fn clear_plan(&self, layer: ScreenLayer) -> Vec<ScreenAssignment> {
        self.fields
            .iter()
            .filter(|field| field.layer == layer)
            .cloned()
            .map(|field| ScreenAssignment {
                value: field.clear_value.clone(),
                field,
            })
            .collect()
    }
}

fn build_field_spec(runtime_field: &RuntimeFieldSpec) -> Result<ScreenFieldSpec> {
    let layer = ScreenLayer::parse(&runtime_field.layer, &runtime_field.name)?;
    let value_kind = ScreenValueKind::parse(&runtime_field.value_kind, &runtime_field.name)?;

    let (public_layer, _) =
        runtime_field
            .name
            .split_once('.')
            .ok_or_else(|| ScreenError::InvalidRuntimeFieldSpec {
                field: runtime_field.name.clone(),
                message: "public field name must use <layer>.<name> syntax".to_string(),
            })?;

    if public_layer != layer.as_str() {
        return Err(ScreenError::InvalidRuntimeFieldSpec {
            field: runtime_field.name.clone(),
            message: format!(
                "public field name prefix `{public_layer}` does not match declared layer `{}`",
                layer.as_str()
            ),
        });
    }

    if runtime_field.runtime_key.trim().is_empty() {
        return Err(ScreenError::InvalidRuntimeFieldSpec {
            field: runtime_field.name.clone(),
            message: "runtime_key must not be empty".to_string(),
        });
    }

    let clear_value = clear_value_for_field(&runtime_field.name)?;
    if clear_value.kind() != value_kind {
        return Err(ScreenError::InvalidRuntimeFieldSpec {
            field: runtime_field.name.clone(),
            message: format!(
                "host clear value kind `{}` does not match manifest value kind `{}`",
                clear_value.kind().expected_description(),
                value_kind.expected_description()
            ),
        });
    }

    Ok(ScreenFieldSpec {
        public_name: runtime_field.name.clone(),
        layer,
        value_kind,
        runtime_key: runtime_field.runtime_key.clone(),
        clear_value,
    })
}

fn clear_value_for_field(field_name: &str) -> Result<ScreenValue> {
    match field_name {
        "persistent.title" | "persistent.bottom" | "slow.message" | "fast.action" => {
            Ok(ScreenValue::Text(String::new()))
        }
        "persistent.value" | "persistent.min" | "persistent.step" | "persistent.bank" => {
            Ok(ScreenValue::Int(0))
        }
        "persistent.max" => Ok(ScreenValue::Int(127)),
        "persistent.default" => Ok(ScreenValue::Int(-1)),
        "persistent.info" => Ok(ScreenValue::TextList(vec![
            DEFAULT_INFO_LABEL.to_string();
            TEXT_LIST_ITEM_COUNT
        ])),
        "persistent.clamp_min" | "persistent.clamp_max" => Ok(ScreenValue::Bool(false)),
        _ => Err(ScreenError::InvalidRuntimeFieldSpec {
            field: field_name.to_string(),
            message: "missing host clear behavior for this curated field".to_string(),
        }),
    }
}

fn parse_value(field: &ScreenFieldSpec, raw_value: &str) -> Result<ScreenValue> {
    match field.value_kind {
        ScreenValueKind::Text => Ok(ScreenValue::Text(raw_value.to_string())),
        ScreenValueKind::Int => {
            raw_value
                .parse::<i32>()
                .map(ScreenValue::Int)
                .map_err(|_| ScreenError::InvalidValue {
                    field: field.public_name.clone(),
                    expected: field.value_kind.expected_description(),
                    actual: raw_value.to_string(),
                })
        }
        ScreenValueKind::Bool => {
            parse_bool(raw_value)
                .map(ScreenValue::Bool)
                .ok_or_else(|| ScreenError::InvalidValue {
                    field: field.public_name.clone(),
                    expected: field.value_kind.expected_description(),
                    actual: raw_value.to_string(),
                })
        }
        ScreenValueKind::TextList => {
            let items = raw_value.split('|').map(str::to_string).collect::<Vec<_>>();

            if items.len() != TEXT_LIST_ITEM_COUNT {
                return Err(ScreenError::InvalidValue {
                    field: field.public_name.clone(),
                    expected: field.value_kind.expected_description(),
                    actual: raw_value.to_string(),
                });
            }

            Ok(ScreenValue::TextList(items))
        }
    }
}

fn parse_bool(raw_value: &str) -> Option<bool> {
    if raw_value.eq_ignore_ascii_case("true") {
        Some(true)
    } else if raw_value.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use crate::protocol::frame_lua;
    use crate::runtime_bundle::{normalized_sha256, RuntimeBundle};

    #[test]
    fn bundled_registry_exposes_typed_field_metadata() {
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let title = registry.field("persistent.title").unwrap();
        assert_eq!(title.layer(), ScreenLayer::Persistent);
        assert_eq!(title.value_kind(), ScreenValueKind::Text);
        assert_eq!(title.runtime_key(), "t");
        assert_eq!(title.clear_value(), &ScreenValue::Text(String::new()));

        let info = registry.field("persistent.info").unwrap();
        assert_eq!(info.value_kind(), ScreenValueKind::TextList);
        assert_eq!(
            info.clear_value(),
            &ScreenValue::TextList(vec![DEFAULT_INFO_LABEL.to_string(); TEXT_LIST_ITEM_COUNT])
        );
    }

    #[test]
    fn parses_screen_assignments_into_typed_values() {
        let registry = ScreenFieldRegistry::bundled().unwrap();
        let assignments = registry
            .parse_assignments([
                "persistent.title=Hello",
                "persistent.value=-7",
                "persistent.clamp_min=true",
                "persistent.info=one|two|three|four|five|six|seven|eight",
            ])
            .unwrap();

        assert_eq!(
            assignments[0].value(),
            &ScreenValue::Text("Hello".to_string())
        );
        assert_eq!(assignments[1].value(), &ScreenValue::Int(-7));
        assert_eq!(assignments[2].value(), &ScreenValue::Bool(true));
        assert_eq!(
            assignments[3].value(),
            &ScreenValue::TextList(vec![
                "one".to_string(),
                "two".to_string(),
                "three".to_string(),
                "four".to_string(),
                "five".to_string(),
                "six".to_string(),
                "seven".to_string(),
                "eight".to_string(),
            ])
        );
    }

    #[test]
    fn rejects_unknown_screen_field_names() {
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let error = registry
            .parse_assignment("persistent.unknown=Hello")
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "unknown screen field `persistent.unknown` (supported fields: persistent.title, persistent.bottom, persistent.value, persistent.min, persistent.max, persistent.default, persistent.step, persistent.info, persistent.clamp_min, persistent.clamp_max, persistent.bank, slow.message, fast.action)"
        );
    }

    #[test]
    fn rejects_invalid_assignment_value_types() {
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let int_error = registry
            .parse_assignment("persistent.value=abc")
            .unwrap_err();
        assert_eq!(
            int_error.to_string(),
            "invalid value for screen field `persistent.value`: expected an integer, got `abc`"
        );

        let bool_error = registry
            .parse_assignment("persistent.clamp_min=maybe")
            .unwrap_err();
        assert_eq!(
            bool_error.to_string(),
            "invalid value for screen field `persistent.clamp_min`: expected a boolean (`true` or `false`), got `maybe`"
        );

        let list_error = registry
            .parse_assignment("persistent.info=one|two|three")
            .unwrap_err();
        assert_eq!(
            list_error.to_string(),
            "invalid value for screen field `persistent.info`: expected 8 pipe-separated text items, got `one|two|three`"
        );
    }

    #[test]
    fn clear_plan_uses_layer_specific_fields_and_runtime_defaults() {
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let persistent = registry.clear_plan(ScreenLayer::Persistent);
        let slow = registry.clear_plan(ScreenLayer::Slow);
        let fast = registry.clear_plan(ScreenLayer::Fast);

        assert_eq!(persistent.len(), 11);
        assert_eq!(slow.len(), 1);
        assert_eq!(fast.len(), 1);
        assert_eq!(slow[0].field().public_name(), "slow.message");
        assert_eq!(slow[0].value(), &ScreenValue::Text(String::new()));
        assert_eq!(fast[0].field().public_name(), "fast.action");
        assert_eq!(fast[0].value(), &ScreenValue::Text(String::new()));

        let max = persistent
            .iter()
            .find(|assignment| assignment.field().public_name() == "persistent.max")
            .unwrap();
        assert_eq!(max.value(), &ScreenValue::Int(127));

        let default = persistent
            .iter()
            .find(|assignment| assignment.field().public_name() == "persistent.default")
            .unwrap();
        assert_eq!(default.value(), &ScreenValue::Int(-1));
    }

    #[test]
    fn rejects_manifest_fields_with_unsupported_value_kinds() {
        let fixture = tempdir().unwrap();
        write_bundle_fixture(
            fixture.path(),
            r#"
[[fields]]
name = "persistent.title"
layer = "persistent"
value_kind = "float"
runtime_key = "t"
notes = "fixture"
"#,
        );

        let bundle = RuntimeBundle::load_from_dir(fixture.path()).unwrap();
        let error = ScreenFieldRegistry::from_bundle(&bundle).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid bundled screen field `persistent.title`: unsupported value kind `float`"
        );
    }

    #[test]
    fn rejects_manifest_fields_with_mismatched_layer_prefixes() {
        let fixture = tempdir().unwrap();
        write_bundle_fixture(
            fixture.path(),
            r#"
[[fields]]
name = "fast.action"
layer = "slow"
value_kind = "text"
runtime_key = "a"
notes = "fixture"
"#,
        );

        let bundle = RuntimeBundle::load_from_dir(fixture.path()).unwrap();
        let error = ScreenFieldRegistry::from_bundle(&bundle).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid bundled screen field `fast.action`: public field name prefix `fast` does not match declared layer `slow`"
        );
    }

    fn write_bundle_fixture(root: &Path, fields: &str) {
        let asset_content = "return 1\n";
        let stored_hash = normalized_sha256(&frame_lua(asset_content));

        fs::write(root.join("lcd-init.lua"), asset_content).unwrap();
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
normalized_sha256 = "{stored_hash}"
runtime_marker = "fixture:lcd-init"

{fields}
"#
            ),
        )
        .unwrap();
    }
}
