use std::collections::{HashMap, HashSet};
use std::error::Error as StdError;
use std::fmt;
use std::path::Path;

use crate::runtime::installed_runtime_dir;
use crate::runtime_bundle::{
    RuntimeBundle, RuntimeBundleError, RuntimeFieldSpec, RuntimeLayerActivation,
};

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
    InvalidActivationMix {
        activate: ScreenLayer,
        field: String,
        field_layer: ScreenLayer,
    },
    UnsupportedActivationLayer {
        layer: ScreenLayer,
    },
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
            Self::InvalidActivationMix {
                activate,
                field,
                field_layer,
            } => write!(
                f,
                "screen set --activate {} only supports {}-layer assignments, but `{field}` belongs to the {} layer",
                activate.as_str(),
                activate.as_str(),
                field_layer.as_str()
            ),
            Self::UnsupportedActivationLayer { layer } => write!(
                f,
                "the current fixed runtime helper contract does not yet support activating manifest layer `{}`",
                layer.as_str()
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
            | Self::InstalledRuntimeMissing
            | Self::InvalidActivationMix { .. }
            | Self::UnsupportedActivationLayer { .. } => None,
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

    let mut statements = Vec::new();

    let persistent = assignments
        .iter()
        .filter(|assignment| assignment.field.layer.as_str() == "persistent")
        .cloned()
        .collect::<Vec<_>>();
    if !persistent.is_empty() {
        statements.push(compile_persistent_update(&persistent)?);
    }

    for assignment in assignments
        .iter()
        .filter(|assignment| assignment.field.layer.as_str() != "persistent")
    {
        statements.push(compile_overlay_update(assignment)?);
    }

    if let Some(layer) = activate {
        if let Some(statement) = compile_activate_lua(layer)? {
            statements.push(statement);
        }
    }

    Ok(statements.join(";"))
}

pub fn compile_clear_lua(registry: &ScreenFieldRegistry, layer: &ScreenLayer) -> Result<String> {
    compile_set_lua(&registry.clear_plan(layer), None)
}

pub fn compile_activate_lua(layer: &ScreenLayer) -> Result<Option<String>> {
    match layer.as_str() {
        "slow" => Ok(Some("A(5)".to_string())),
        "fast" => Ok(Some("A(1)".to_string())),
        "persistent" => Ok(None),
        _ => Err(ScreenError::UnsupportedActivationLayer {
            layer: layer.clone(),
        }),
    }
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

fn validate_assignments(
    assignments: &[ScreenAssignment],
    activate: Option<&ScreenLayer>,
) -> Result<()> {
    let mut seen = HashSet::with_capacity(assignments.len());
    for assignment in assignments {
        if !seen.insert(assignment.field.public_name.clone()) {
            return Err(ScreenError::DuplicateFieldAssignment {
                field: assignment.field.public_name.clone(),
            });
        }
    }

    if let Some(activate) = activate {
        for assignment in assignments {
            if &assignment.field.layer != activate {
                return Err(ScreenError::InvalidActivationMix {
                    activate: activate.clone(),
                    field: assignment.field.public_name.clone(),
                    field_layer: assignment.field.layer.clone(),
                });
            }
        }
    }

    Ok(())
}

fn compile_persistent_update(assignments: &[ScreenAssignment]) -> Result<String> {
    let mut args = [
        String::from("nil"),
        String::from("nil"),
        String::from("nil"),
        String::from("nil"),
        String::from("nil"),
        String::from("nil"),
        String::from("nil"),
        String::from("nil"),
        String::from("nil"),
        String::from("nil"),
    ];
    let mut clamp_min = None;
    let mut clamp_max = None;

    for assignment in assignments {
        match assignment.field.runtime_key.as_str() {
            "v" => args[0] = compile_lua_value(&assignment.value),
            "n" => args[1] = compile_lua_value(&assignment.value),
            "x" => args[2] = compile_lua_value(&assignment.value),
            "t" => args[3] = compile_lua_value(&assignment.value),
            "b" => args[4] = compile_lua_value(&assignment.value),
            "s" => args[5] = compile_lua_value(&assignment.value),
            "d" => args[6] = compile_lua_value(&assignment.value),
            "i" => args[7] = compile_lua_value(&assignment.value),
            "l" => clamp_min = Some(compile_lua_value(&assignment.value)),
            "h" => clamp_max = Some(compile_lua_value(&assignment.value)),
            "k" => args[9] = compile_lua_value(&assignment.value),
            runtime_key => {
                return Err(ScreenError::InvalidRuntimeFieldSpec {
                    field: assignment.field.public_name.clone(),
                    message: format!(
                    "unsupported persistent runtime key `{runtime_key}` for host Lua compilation"
                ),
                })
            }
        }
    }

    if clamp_min.is_some() || clamp_max.is_some() {
        args[8] = format!(
            "{{{},{}}}",
            clamp_min.unwrap_or_else(|| "nil".to_string()),
            clamp_max.unwrap_or_else(|| "nil".to_string())
        );
    }

    Ok(format!("P({})", args.join(",")))
}

fn compile_overlay_update(assignment: &ScreenAssignment) -> Result<String> {
    match (
        assignment.field.layer.as_str(),
        assignment.field.runtime_key.as_str(),
        &assignment.value,
    ) {
        ("slow", "m", ScreenValue::Text(_)) => {
            Ok(format!("S({})", compile_lua_value(&assignment.value)))
        }
        ("fast", "a", ScreenValue::Text(_)) => {
            Ok(format!("F({})", compile_lua_value(&assignment.value)))
        }
        (layer, runtime_key, _) => Err(ScreenError::InvalidRuntimeFieldSpec {
            field: assignment.field.public_name.clone(),
            message: format!(
                "unsupported {}-layer runtime mapping `{runtime_key}` for host Lua compilation",
                layer
            ),
        }),
    }
}

fn compile_lua_value(value: &ScreenValue) -> String {
    match value {
        ScreenValue::Text(text) => quote_lua_string(text),
        ScreenValue::Int(value) => value.to_string(),
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

        let info = registry.field("persistent.info").unwrap();
        assert_eq!(info.value_kind(), ScreenValueKind::TextList);
        assert_eq!(
            info.clear_value(),
            &ScreenValue::TextList(vec![DEFAULT_INFO_LABEL.to_string(); TEXT_LIST_ITEM_COUNT])
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

        assert_eq!(lua, "P(42,nil,nil,'Hello',nil,nil,nil,nil,{true,nil},nil)");
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
            "P(nil,nil,nil,'Hello',nil,nil,nil,nil,nil,nil);S('Disk almost full');F('Tap')"
        );
    }

    #[test]
    fn compiles_overlay_only_updates_with_dirty_marking() {
        let registry = ScreenFieldRegistry::bundled().unwrap();
        let assignments = registry
            .parse_assignments(["slow.message=Disk almost full"])
            .unwrap();

        let lua = compile_set_lua(&assignments, None).unwrap();

        assert_eq!(lua, "S('Disk almost full')");
    }

    #[test]
    fn rejects_mixed_layer_updates_when_activation_is_requested() {
        let registry = ScreenFieldRegistry::bundled().unwrap();
        let assignments = registry
            .parse_assignments(["persistent.title=Hello", "slow.message=Disk almost full"])
            .unwrap();

        let activate = ScreenLayer::new("slow");
        let error = compile_set_lua(&assignments, Some(&activate)).unwrap_err();

        assert_eq!(
            error.to_string(),
            "screen set --activate slow only supports slow-layer assignments, but `persistent.title` belongs to the persistent layer"
        );
    }

    #[test]
    fn compiles_layer_clear_using_runtime_defaults() {
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let slow_lua = compile_clear_lua(&registry, &ScreenLayer::new("slow")).unwrap();
        let persistent_lua = compile_clear_lua(&registry, &ScreenLayer::new("persistent")).unwrap();

        assert_eq!(slow_lua, "S('')");
        assert_eq!(
            persistent_lua,
            "P(0,0,127,'','',0,-1,{'---','---','---','---','---','---','---','---'},{false,false},0)"
        );
    }

    #[test]
    fn compiles_current_fixed_runtime_activation_helpers() {
        assert_eq!(
            compile_activate_lua(&ScreenLayer::new("slow")).unwrap(),
            Some("A(5)".to_string())
        );
        assert_eq!(
            compile_activate_lua(&ScreenLayer::new("fast")).unwrap(),
            Some("A(1)".to_string())
        );
        assert_eq!(
            compile_activate_lua(&ScreenLayer::new("persistent")).unwrap(),
            None
        );

        let error = compile_activate_lua(&ScreenLayer::new("alt")).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the current fixed runtime helper contract does not yet support activating manifest layer `alt`"
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

        fs::write(root.join("lcd-init.lua"), asset_content).unwrap();
        fs::write(
            root.join("manifest.toml"),
            format!(
                r#"
bundle_version = "test"
compatibility_reference = "fixture"
runtime_marker = "fixture"

[[layers]]
name = "persistent"
priority = 0
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

[[owned_slots]]
name = "lcd-init"
page = 0
element = 13
event = 0
asset = "lcd-init.lua"
install_order = 10
runtime_marker = "fixture:lcd-init"

{fields}
"#
            ),
        )
        .unwrap();
    }
}
