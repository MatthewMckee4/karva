//! Generate a JSON Schema for `karva.toml`.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use karva_metadata::{Config, Options, OverrideOptions};
use ruff_options_metadata::{OptionField, OptionSet, OptionsMetadata, Visit};
use serde_json::{Map, Value, json};

use crate::{Mode, apply_mode};

#[derive(clap::Args)]
pub struct Args {
    /// Write the generated schema to stdout (rather than to `karva.schema.json`).
    #[arg(long, default_value_t, value_enum)]
    pub mode: Mode,
}

pub fn main(args: &Args) -> Result<()> {
    let generated = serde_json::to_string_pretty(&generate()?)? + "\n";
    apply_mode(args.mode, "karva.schema.json", &generated)
}

fn generate() -> Result<Value> {
    let mut root = schema_for_set(Config::metadata())?;
    properties_mut(&mut root)?.insert("profile".to_string(), profile_map_schema()?);

    let object = object_mut(&mut root)?;
    object.insert(
        "$schema".to_string(),
        json!("https://json-schema.org/draft/2020-12/schema"),
    );
    object.insert("title".to_string(), json!("Karva configuration"));
    Ok(root)
}

fn profile_map_schema() -> Result<Value> {
    Ok(json!({
        "type": "object",
        "propertyNames": {
            "pattern": "^[A-Za-z0-9_-]+$",
            "not": { "pattern": "^default-" }
        },
        "additionalProperties": profile_schema()?
    }))
}

fn profile_schema() -> Result<Value> {
    let mut schema = schema_for_set(Options::metadata())?;
    properties_mut(&mut schema)?.insert(
        "overrides".to_string(),
        json!({
            "type": "array",
            "items": override_schema()?,
            "default": []
        }),
    );
    Ok(schema)
}

fn override_schema() -> Result<Value> {
    let mut schema = schema_for_set(OverrideOptions::metadata())?;
    object_mut(&mut schema)?.insert("required".to_string(), json!(["filter"]));
    Ok(schema)
}

fn schema_for_set(set: OptionSet) -> Result<Value> {
    let mut visitor = CollectOptionsVisitor::default();
    set.record(&mut visitor);

    let mut properties = BTreeMap::new();
    for (name, field) in visitor.fields {
        properties.insert(name, schema_for_field(&field)?);
    }
    for (name, group) in visitor.groups {
        properties.insert(name, schema_for_set(group)?);
    }

    let mut schema = json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false
    });
    if let Some(description) = set.documentation() {
        object_mut(&mut schema)?.insert("description".to_string(), json!(description));
    }
    Ok(schema)
}

fn schema_for_field(field: &OptionField) -> Result<Value> {
    let mut schema = schema_for_value_type(field.value_type)?;
    let object = object_mut(&mut schema)?;
    object.insert("description".to_string(), json!(field.doc));
    if field.default != "required" {
        object.insert("default".to_string(), default_value(field)?);
    }
    Ok(schema)
}

fn schema_for_value_type(value_type: &str) -> Result<Value> {
    let schema = match value_type {
        "bool" | "true | false" => json!({ "type": "boolean" }),
        "list[str]" => json!({ "type": "array", "items": { "type": "string" } }),
        "path" | "string" => json!({ "type": "string" }),
        "positive integer" => json!({ "type": "integer", "minimum": 1 }),
        "u32" => json!({ "type": "integer", "minimum": 0 }),
        "float (seconds)" => json!({ "type": "number" }),
        "float (0..=100)" => json!({ "type": "number", "minimum": 0, "maximum": 100 }),
        value_type if value_type.contains(" | ") => json!({
            "type": "string",
            "enum": value_type.split(" | ").collect::<Vec<_>>()
        }),
        _ => bail!("unsupported option value type `{value_type}` in JSON Schema generator"),
    };
    Ok(schema)
}

fn default_value(field: &OptionField) -> Result<Value> {
    let value = match field.default {
        "null" => Value::Null,
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        default
            if default.starts_with('"')
                || (matches!(
                    field.value_type,
                    "positive integer" | "u32" | "float (seconds)" | "float (0..=100)"
                ) && default != "unlimited") =>
        {
            serde_json::from_str(default)?
        }
        default => json!(default),
    };
    Ok(value)
}

fn object_mut(value: &mut Value) -> Result<&mut Map<String, Value>> {
    value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("generated schema node is not an object"))
}

fn properties_mut(value: &mut Value) -> Result<&mut Map<String, Value>> {
    object_mut(value)?
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("generated object schema has no properties"))
}

#[derive(Default)]
struct CollectOptionsVisitor {
    fields: Vec<(String, OptionField)>,
    groups: Vec<(String, OptionSet)>,
}

impl Visit for CollectOptionsVisitor {
    fn record_field(&mut self, name: &str, field: OptionField) {
        self.fields.push((name.to_string(), field));
    }

    fn record_set(&mut self, name: &str, group: OptionSet) {
        self.groups.push((name.to_string(), group));
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::{Args, main};
    use crate::Mode;

    #[test]
    #[cfg(unix)]
    fn json_schema_up_to_date() -> Result<()> {
        main(&Args { mode: Mode::Check })
    }
}
