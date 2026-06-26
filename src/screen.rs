use std::collections::{HashMap, HashSet};
use std::error::Error as StdError;
use std::fmt;
use std::path::Path;

use toml::Value as TomlValue;

use crate::runtime::installed_runtime_dir;
use crate::runtime_bundle::{
    RuntimeBundle, RuntimeBundleError, RuntimeFieldSpec, RuntimeLayerActivation,
};

const TEXT_LIST_ITEM_COUNT: usize = 8;

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
    UnknownLayer {
        name: String,
        supported: Vec<String>,
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
    DuplicateFieldAssignment {
        field: String,
    },
    InstalledRuntimeMissing,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScreenLayer(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenLayerSpec {
    name: ScreenLayer,
    activation: RuntimeLayerActivation,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ScreenValueKind {
    Text,
    Int,
    Float,
    Bool,
    TextList,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScreenValue {
    Text(String),
    Int(i32),
    Float(f64),
    Bool(bool),
    TextList(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScreenFieldSpec {
    public_name: String,
    layer: ScreenLayer,
    value_kind: ScreenValueKind,
    runtime_key: String,
    clear_value: ScreenValue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScreenAssignment {
    field: ScreenFieldSpec,
    value: ScreenValue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScreenFieldRegistry {
    layers: Vec<ScreenLayerSpec>,
    layers_by_name: HashMap<String, usize>,
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
                write!(
                    f,
                    "screen assignment `{input}` must use FIELD=VALUE syntax; run `vsn1-cli screen set --help` for examples"
                )
            }
            Self::UnknownLayer { name, supported } => write!(
                f,
                "unknown screen layer `{name}` (supported layers: {}); run `vsn1-cli screen --help` for examples",
                supported.join(", ")
            ),
            Self::UnknownField { name, supported } => write!(
                f,
                "unknown screen field `{name}` (supported fields: {}); run `vsn1-cli screen set --help` for examples",
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
            Self::DuplicateFieldAssignment { field } => write!(
                f,
                "screen field `{field}` was assigned more than once in the same command"
            ),
            Self::InstalledRuntimeMissing => write!(
                f,
                "no frozen installed runtime was found under ~/.config/vsn1-cli/runtime; run `vsn1-cli runtime install <name>` first"
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
            | Self::UnknownLayer { .. }
            | Self::UnknownField { .. }
            | Self::InvalidValue { .. }
            | Self::DuplicateFieldAssignment { .. }
            | Self::InstalledRuntimeMissing => None,
        }
    }
}

impl From<RuntimeBundleError> for ScreenError {
    fn from(value: RuntimeBundleError) -> Self {
        Self::Bundle(value)
    }
}

impl ScreenLayer {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ScreenLayerSpec {
    pub fn name(&self) -> &ScreenLayer {
        &self.name
    }

    pub fn activation(&self) -> RuntimeLayerActivation {
        self.activation
    }

    pub fn timeout_ms(&self) -> Option<u64> {
        self.timeout_ms
    }
}

impl ScreenValueKind {
    pub fn expected_description(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Int => "an integer",
            Self::Float => "a number",
            Self::Bool => "a boolean (`true` or `false`)",
            Self::TextList => "8 pipe-separated text items",
        }
    }

    fn parse(raw: &str, field_name: &str) -> Result<Self> {
        match raw {
            "text" => Ok(Self::Text),
            "int" => Ok(Self::Int),
            "float" => Ok(Self::Float),
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
            Self::Float(_) => ScreenValueKind::Float,
            Self::Bool(_) => ScreenValueKind::Bool,
            Self::TextList(_) => ScreenValueKind::TextList,
        }
    }
}

impl ScreenFieldSpec {
    pub fn public_name(&self) -> &str {
        &self.public_name
    }

    pub fn layer(&self) -> &ScreenLayer {
        &self.layer
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
    pub fn installed() -> Result<Self> {
        Self::from_optional_runtime_dir(installed_runtime_dir().as_deref())
    }

    pub fn bundled() -> Result<Self> {
        let bundle = RuntimeBundle::bundled()?;
        Self::from_bundle(&bundle)
    }

    fn from_optional_runtime_dir(runtime_dir: Option<&Path>) -> Result<Self> {
        let Some(runtime_dir) = runtime_dir.filter(|path| path.is_dir()) else {
            return Err(ScreenError::InstalledRuntimeMissing);
        };

        let bundle = RuntimeBundle::load_from_dir(runtime_dir)?;
        Self::from_bundle(&bundle)
    }

    pub fn from_bundle(bundle: &RuntimeBundle) -> Result<Self> {
        let mut layers = Vec::with_capacity(bundle.manifest().layers.len());
        let mut layers_by_name = HashMap::with_capacity(bundle.manifest().layers.len());

        for runtime_layer in &bundle.manifest().layers {
            let layer = ScreenLayer::new(runtime_layer.name.clone());
            let layer_name = layer.as_str().to_string();
            let layer_index = layers.len();

            if layers_by_name
                .insert(layer_name.clone(), layer_index)
                .is_some()
            {
                return Err(ScreenError::InvalidRuntimeFieldSpec {
                    field: layer_name,
                    message: "duplicate layer name after host registry conversion".to_string(),
                });
            }

            layers.push(ScreenLayerSpec {
                name: layer,
                activation: runtime_layer.activation,
                timeout_ms: runtime_layer.timeout_ms,
            });
        }

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
            layers,
            layers_by_name,
            fields,
            fields_by_name,
        })
    }

    pub fn layers(&self) -> &[ScreenLayerSpec] {
        &self.layers
    }

    pub fn layer(&self, name: &str) -> Result<&ScreenLayerSpec> {
        self.layers_by_name
            .get(name)
            .map(|index| &self.layers[*index])
            .ok_or_else(|| ScreenError::UnknownLayer {
                name: name.to_string(),
                supported: self
                    .layers
                    .iter()
                    .map(|layer| layer.name.as_str().to_string())
                    .collect(),
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

    pub fn fields_for_layer(&self, layer: &ScreenLayer) -> Vec<&ScreenFieldSpec> {
        self.fields
            .iter()
            .filter(|field| &field.layer == layer)
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

    pub fn clear_plan(&self, layer: &ScreenLayer) -> Vec<ScreenAssignment> {
        self.fields
            .iter()
            .filter(|field| &field.layer == layer)
            .cloned()
            .map(|field| ScreenAssignment {
                value: field.clear_value.clone(),
                field,
            })
            .collect()
    }
}

pub fn compile_set_lua(
    assignments: &[ScreenAssignment],
    activate: Option<&ScreenLayer>,
) -> Result<String> {
    validate_assignments(assignments, activate)?;

    let mut statements = assignments
        .iter()
        .map(compile_field_update)
        .collect::<Vec<_>>();

    if let Some(layer) = activate {
        statements.push(compile_activate_lua(layer)?);
    }

    Ok(statements.join(";"))
}

pub fn compile_clear_lua(registry: &ScreenFieldRegistry, layer: &ScreenLayer) -> Result<String> {
    compile_set_lua(&registry.clear_plan(layer), None)
}

pub fn compile_activate_lua(layer: &ScreenLayer) -> Result<String> {
    Ok(format!(
        "activate_layer({})",
        quote_lua_string(layer.as_str())
    ))
}

fn build_field_spec(runtime_field: &RuntimeFieldSpec) -> Result<ScreenFieldSpec> {
    let layer = ScreenLayer::new(runtime_field.layer.clone());
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

    let clear_value =
        parse_manifest_clear_value(&runtime_field.name, value_kind, &runtime_field.clear_value)?;

    Ok(ScreenFieldSpec {
        public_name: runtime_field.name.clone(),
        layer,
        value_kind,
        runtime_key: runtime_field.runtime_key.clone(),
        clear_value,
    })
}

fn parse_manifest_clear_value(
    field_name: &str,
    value_kind: ScreenValueKind,
    clear_value: &TomlValue,
) -> Result<ScreenValue> {
    match value_kind {
        ScreenValueKind::Text => clear_value
            .as_str()
            .map(|value| ScreenValue::Text(value.to_string())),
        ScreenValueKind::Int => clear_value
            .as_integer()
            .and_then(|value| i32::try_from(value).ok())
            .map(ScreenValue::Int),
        ScreenValueKind::Float => clear_value
            .as_float()
            .or_else(|| clear_value.as_integer().map(|value| value as f64))
            .map(ScreenValue::Float),
        ScreenValueKind::Bool => clear_value.as_bool().map(ScreenValue::Bool),
        ScreenValueKind::TextList => clear_value.as_array().and_then(|items| {
            if items.len() != TEXT_LIST_ITEM_COUNT {
                return None;
            }

            items
                .iter()
                .map(|item| item.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
                .map(ScreenValue::TextList)
        }),
    }
    .ok_or_else(|| ScreenError::InvalidRuntimeFieldSpec {
        field: field_name.to_string(),
        message: format!(
            "manifest clear_value does not match declared value kind `{}`",
            value_kind.expected_description()
        ),
    })
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
        ScreenValueKind::Float => raw_value
            .parse::<f64>()
            .map(ScreenValue::Float)
            .map_err(|_| ScreenError::InvalidValue {
                field: field.public_name.clone(),
                expected: field.value_kind.expected_description(),
                actual: raw_value.to_string(),
            }),
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

fn validate_assignments(
    assignments: &[ScreenAssignment],
    _activate: Option<&ScreenLayer>,
) -> Result<()> {
    let mut seen = HashSet::with_capacity(assignments.len());
    for assignment in assignments {
        if !seen.insert(assignment.field.public_name.clone()) {
            return Err(ScreenError::DuplicateFieldAssignment {
                field: assignment.field.public_name.clone(),
            });
        }
    }

    Ok(())
}

fn compile_field_update(assignment: &ScreenAssignment) -> String {
    format!(
        "set_field({},{},{})",
        quote_lua_string(assignment.field.layer.as_str()),
        quote_lua_string(assignment.field.runtime_key.as_str()),
        compile_lua_value(&assignment.value)
    )
}

fn compile_lua_value(value: &ScreenValue) -> String {
    match value {
        ScreenValue::Text(text) => quote_lua_string(text),
        ScreenValue::Int(value) => value.to_string(),
        ScreenValue::Float(value) => value.to_string(),
        ScreenValue::Bool(value) => value.to_string(),
        ScreenValue::TextList(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|item| quote_lua_string(item))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn quote_lua_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('\'');

    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\'' => escaped.push_str("\\'"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(character),
        }
    }

    escaped.push('\'');
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use crate::runtime_bundle::RuntimeBundle;

    #[test]
    fn bundled_registry_exposes_typed_field_metadata() {
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let title = registry.field("persistent.title").unwrap();
        assert_eq!(title.layer(), &ScreenLayer::new("persistent"));
        assert_eq!(title.value_kind(), ScreenValueKind::Text);
        assert_eq!(title.runtime_key(), "t");
        assert_eq!(title.clear_value(), &ScreenValue::Text(String::new()));

        let media = ScreenFieldRegistry::from_bundle(
            &RuntimeBundle::load_from_dir(
                crate::runtime_bundle::bundled_runtime_root_dir().join("media"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            media.field("base.duration").unwrap().value_kind(),
            ScreenValueKind::Float
        );

        let info = registry.field("persistent.info").unwrap();
        assert_eq!(info.value_kind(), ScreenValueKind::TextList);
        assert_eq!(
            info.clear_value(),
            &ScreenValue::TextList(vec!["---".to_string(); TEXT_LIST_ITEM_COUNT])
        );
    }

    #[test]
    fn installed_registry_loads_from_runtime_copy() {
        let fixture = tempdir().unwrap();
        write_bundle_fixture(
            fixture.path(),
            r#"
[[fields]]
name = "persistent.title"
layer = "persistent"
value_kind = "text"
runtime_key = "t"
clear_value = ""
notes = "fixture"
"#,
        );

        let registry =
            ScreenFieldRegistry::from_optional_runtime_dir(Some(fixture.path())).unwrap();
        assert_eq!(
            registry.field("persistent.title").unwrap().runtime_key(),
            "t"
        );
    }

    #[test]
    fn file_manager_runtime_variant_compiles_the_same_screen_helpers_as_default() {
        let default_registry = ScreenFieldRegistry::bundled().unwrap();
        let file_manager_registry = ScreenFieldRegistry::from_bundle(
            &RuntimeBundle::load_from_dir(
                crate::runtime_bundle::bundled_runtime_root_dir().join("default-file-manager-poc"),
            )
            .unwrap(),
        )
        .unwrap();

        let default_assignments = default_registry
            .parse_assignments(["persistent.title=Tempo", "slow.message=Disk almost full"])
            .unwrap();
        let file_manager_assignments = file_manager_registry
            .parse_assignments(["persistent.title=Tempo", "slow.message=Disk almost full"])
            .unwrap();

        assert_eq!(
            default_registry
                .layers()
                .iter()
                .map(|layer| {
                    (
                        layer.name().as_str(),
                        layer.activation(),
                        layer.timeout_ms(),
                    )
                })
                .collect::<Vec<_>>(),
            file_manager_registry
                .layers()
                .iter()
                .map(|layer| {
                    (
                        layer.name().as_str(),
                        layer.activation(),
                        layer.timeout_ms(),
                    )
                })
                .collect::<Vec<_>>()
        );
        assert_eq!(
            default_registry
                .fields()
                .iter()
                .map(|field| field.public_name())
                .collect::<Vec<_>>(),
            file_manager_registry
                .fields()
                .iter()
                .map(|field| field.public_name())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            compile_set_lua(&default_assignments, Some(&ScreenLayer::new("slow"))).unwrap(),
            compile_set_lua(&file_manager_assignments, Some(&ScreenLayer::new("slow"))).unwrap()
        );
        assert_eq!(
            compile_clear_lua(&default_registry, &ScreenLayer::new("persistent")).unwrap(),
            compile_clear_lua(&file_manager_registry, &ScreenLayer::new("persistent")).unwrap()
        );
        assert_eq!(
            compile_activate_lua(&ScreenLayer::new("fast")).unwrap(),
            "activate_layer('fast')"
        );
    }

    #[test]
    fn installed_registry_requires_runtime_copy() {
        let error = ScreenFieldRegistry::from_optional_runtime_dir(None).unwrap_err();

        assert_eq!(
            error.to_string(),
            "no frozen installed runtime was found under ~/.config/vsn1-cli/runtime; run `vsn1-cli runtime install <name>` first"
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
    fn parses_float_screen_assignments_into_typed_values() {
        let media = ScreenFieldRegistry::from_bundle(
            &RuntimeBundle::load_from_dir(
                crate::runtime_bundle::bundled_runtime_root_dir().join("media"),
            )
            .unwrap(),
        )
        .unwrap();
        let assignments = media
            .parse_assignments(["base.duration=344.5", "base.position=91.25"])
            .unwrap();

        assert_eq!(assignments[0].value(), &ScreenValue::Float(344.5));
        assert_eq!(assignments[1].value(), &ScreenValue::Float(91.25));
    }

    #[test]
    fn rejects_unknown_screen_field_names() {
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let error = registry
            .parse_assignment("persistent.unknown=Hello")
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "unknown screen field `persistent.unknown` (supported fields: persistent.title, persistent.bottom, persistent.value, persistent.min, persistent.max, persistent.default, persistent.step, persistent.info, persistent.clamp_min, persistent.clamp_max, persistent.bank, slow.message, fast.action); run `vsn1-cli screen set --help` for examples"
        );
    }

    #[test]
    fn parses_text_values_with_embedded_equals_signs() {
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let assignment = registry
            .parse_assignment("persistent.title=left=right")
            .unwrap();

        assert_eq!(
            assignment.value(),
            &ScreenValue::Text("left=right".to_string())
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

        let media = ScreenFieldRegistry::from_bundle(
            &RuntimeBundle::load_from_dir(
                crate::runtime_bundle::bundled_runtime_root_dir().join("media"),
            )
            .unwrap(),
        )
        .unwrap();
        let float_error = media.parse_assignment("base.duration=abc").unwrap_err();
        assert_eq!(
            float_error.to_string(),
            "invalid value for screen field `base.duration`: expected a number, got `abc`"
        );
    }

    #[test]
    fn clear_plan_uses_layer_specific_fields_and_runtime_defaults() {
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let persistent = registry.clear_plan(&ScreenLayer::new("persistent"));
        let slow = registry.clear_plan(&ScreenLayer::new("slow"));
        let fast = registry.clear_plan(&ScreenLayer::new("fast"));

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
    fn compiles_partial_persistent_updates_without_resetting_other_fields() {
        let registry = ScreenFieldRegistry::bundled().unwrap();
        let assignments = registry
            .parse_assignments([
                "persistent.title=Hello",
                "persistent.value=42",
                "persistent.clamp_min=true",
            ])
            .unwrap();

        let lua = compile_set_lua(&assignments, None).unwrap();

        assert_eq!(
            lua,
            "set_field('persistent','t','Hello');set_field('persistent','v',42);set_field('persistent','l',true)"
        );
    }

    #[test]
    fn compiles_mixed_layer_updates_into_one_runtime_helper_script() {
        let registry = ScreenFieldRegistry::bundled().unwrap();
        let assignments = registry
            .parse_assignments([
                "persistent.title=Hello",
                "slow.message=Disk almost full",
                "fast.action=Tap",
            ])
            .unwrap();

        let lua = compile_set_lua(&assignments, None).unwrap();

        assert_eq!(
            lua,
            "set_field('persistent','t','Hello');set_field('slow','m','Disk almost full');set_field('fast','a','Tap')"
        );
    }

    #[test]
    fn compiles_single_layer_updates_with_generic_runtime_helpers() {
        let registry = ScreenFieldRegistry::bundled().unwrap();
        let assignments = registry
            .parse_assignments(["slow.message=Disk almost full"])
            .unwrap();

        let lua = compile_set_lua(&assignments, None).unwrap();

        assert_eq!(lua, "set_field('slow','m','Disk almost full')");
    }

    #[test]
    fn compiles_float_updates_with_generic_runtime_helpers() {
        let media = ScreenFieldRegistry::from_bundle(
            &RuntimeBundle::load_from_dir(
                crate::runtime_bundle::bundled_runtime_root_dir().join("media"),
            )
            .unwrap(),
        )
        .unwrap();
        let assignments = media
            .parse_assignments(["base.duration=344.5", "base.position=91.25"])
            .unwrap();

        let lua = compile_set_lua(&assignments, None).unwrap();

        assert_eq!(
            lua,
            "set_field('base','d',344.5);set_field('base','p',91.25)"
        );
    }

    #[test]
    fn compiles_set_and_activate_with_the_generic_layer_contract() {
        let registry = ScreenFieldRegistry::bundled().unwrap();
        let assignments = registry
            .parse_assignments(["slow.message=Disk almost full"])
            .unwrap();

        let lua = compile_set_lua(&assignments, Some(&ScreenLayer::new("slow"))).unwrap();

        assert_eq!(
            lua,
            "set_field('slow','m','Disk almost full');activate_layer('slow')"
        );
    }

    #[test]
    fn compiles_mixed_layer_updates_and_activation_in_one_command() {
        let registry = ScreenFieldRegistry::bundled().unwrap();
        let assignments = registry
            .parse_assignments(["persistent.title=Hello", "slow.message=Disk almost full"])
            .unwrap();

        let lua = compile_set_lua(&assignments, Some(&ScreenLayer::new("slow"))).unwrap();

        assert_eq!(
            lua,
            "set_field('persistent','t','Hello');set_field('slow','m','Disk almost full');activate_layer('slow')"
        );
    }

    #[test]
    fn compiles_layer_clear_using_runtime_defaults() {
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let slow_lua = compile_clear_lua(&registry, &ScreenLayer::new("slow")).unwrap();
        let persistent_lua = compile_clear_lua(&registry, &ScreenLayer::new("persistent")).unwrap();

        assert_eq!(slow_lua, "set_field('slow','m','')");
        assert_eq!(
            persistent_lua,
            "set_field('persistent','t','');set_field('persistent','b','');set_field('persistent','v',0);set_field('persistent','n',0);set_field('persistent','x',127);set_field('persistent','d',-1);set_field('persistent','s',0);set_field('persistent','i',{'---','---','---','---','---','---','---','---'});set_field('persistent','l',false);set_field('persistent','h',false);set_field('persistent','k',0)"
        );
    }

    #[test]
    fn compiles_generic_runtime_activation_helpers() {
        assert_eq!(
            compile_activate_lua(&ScreenLayer::new("slow")).unwrap(),
            "activate_layer('slow')"
        );
        assert_eq!(
            compile_activate_lua(&ScreenLayer::new("fast")).unwrap(),
            "activate_layer('fast')"
        );
        assert_eq!(
            compile_activate_lua(&ScreenLayer::new("persistent")).unwrap(),
            "activate_layer('persistent')"
        );
        assert_eq!(
            compile_activate_lua(&ScreenLayer::new("alt")).unwrap(),
            "activate_layer('alt')"
        );
    }

    #[test]
    fn registry_exposes_runtime_layer_inventory() {
        let registry = ScreenFieldRegistry::bundled().unwrap();

        assert_eq!(
            registry
                .layers()
                .iter()
                .map(|layer| layer.name().as_str())
                .collect::<Vec<_>>(),
            vec!["persistent", "slow", "fast"]
        );
        assert_eq!(
            registry.layer("slow").unwrap().activation(),
            RuntimeLayerActivation::Temporary
        );
        assert_eq!(registry.layer("slow").unwrap().timeout_ms(), Some(5000));
    }

    #[test]
    fn rejects_unknown_screen_layers() {
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let error = registry.layer("notice").unwrap_err();

        assert_eq!(
            error.to_string(),
            "unknown screen layer `notice` (supported layers: persistent, slow, fast); run `vsn1-cli screen --help` for examples"
        );
    }

    #[test]
    fn rejects_duplicate_field_assignments() {
        let registry = ScreenFieldRegistry::bundled().unwrap();
        let assignments = registry
            .parse_assignments(["slow.message=one", "slow.message=two"])
            .unwrap();

        let error = compile_set_lua(&assignments, None).unwrap_err();

        assert_eq!(
            error.to_string(),
            "screen field `slow.message` was assigned more than once in the same command"
        );
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
value_kind = "decimal"
runtime_key = "t"
clear_value = ""
notes = "fixture"
"#,
        );

        let bundle = RuntimeBundle::load_from_dir(fixture.path()).unwrap();
        let error = ScreenFieldRegistry::from_bundle(&bundle).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid bundled screen field `persistent.title`: unsupported value kind `decimal`"
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
clear_value = ""
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

    #[test]
    fn supports_manifest_defined_clear_values_for_non_legacy_field_names() {
        let fixture = tempdir().unwrap();
        write_bundle_fixture(
            fixture.path(),
            r#"
[[fields]]
name = "base.duration"
layer = "base"
value_kind = "int"
runtime_key = "d"
clear_value = 0
notes = "fixture"

[[fields]]
name = "playback_status.status"
layer = "playback_status"
value_kind = "text"
runtime_key = "s"
clear_value = ""
notes = "fixture"
"#,
        );

        let bundle = RuntimeBundle::load_from_dir(fixture.path()).unwrap();
        let registry = ScreenFieldRegistry::from_bundle(&bundle).unwrap();

        assert_eq!(
            registry.field("base.duration").unwrap().clear_value(),
            &ScreenValue::Int(0)
        );
        assert_eq!(
            registry
                .field("playback_status.status")
                .unwrap()
                .clear_value(),
            &ScreenValue::Text(String::new())
        );
    }

    #[test]
    fn supports_manifest_defined_float_clear_values() {
        let fixture = tempdir().unwrap();
        write_bundle_fixture(
            fixture.path(),
            r#"
[[fields]]
name = "base.position"
layer = "base"
value_kind = "float"
runtime_key = "p"
clear_value = 0.0
notes = "fixture"
"#,
        );

        let bundle = RuntimeBundle::load_from_dir(fixture.path()).unwrap();
        let registry = ScreenFieldRegistry::from_bundle(&bundle).unwrap();

        assert_eq!(
            registry.field("base.position").unwrap().clear_value(),
            &ScreenValue::Float(0.0)
        );
    }

    #[test]
    fn rejects_manifest_clear_values_with_the_wrong_type() {
        let fixture = tempdir().unwrap();
        write_bundle_fixture(
            fixture.path(),
            r#"
[[fields]]
name = "base.duration"
layer = "base"
value_kind = "int"
runtime_key = "d"
clear_value = "zero"
notes = "fixture"
"#,
        );

        let bundle = RuntimeBundle::load_from_dir(fixture.path()).unwrap();
        let error = ScreenFieldRegistry::from_bundle(&bundle).unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid bundled screen field `base.duration`: manifest clear_value does not match declared value kind `an integer`"
        );
    }

    fn write_bundle_fixture(root: &Path, fields: &str) {
        let asset_content = "return 1\n";

        fs::write(root.join("lcd-init.lua"), asset_content).unwrap();
        fs::write(
            root.join("manifest.toml"),
            format!(
                r#"
[[layers]]
name = "persistent"
priority = 0
activation = "persistent"

[[layers]]
name = "base"
priority = 1
activation = "persistent"

[[layers]]
name = "slow"
priority = 10
activation = "temporary"
timeout_ms = 5000

[[layers]]
name = "fast"
priority = 20
activation = "temporary"
timeout_ms = 1000

[[layers]]
name = "playback_status"
priority = 30
activation = "temporary"
timeout_ms = 2000

[[owned_slots]]
name = "lcd-init"
page = 0
element = 13
event = 0
asset = "lcd-init.lua"
install_order = 10

{fields}
"#
            ),
        )
        .unwrap();
    }
}
