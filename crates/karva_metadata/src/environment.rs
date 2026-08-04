//! Profile-scoped environment variable configuration.

use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Validated environment variable name safe to pass to worker processes.
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct EnvironmentVariableName(String);

impl EnvironmentVariableName {
    /// Returns the configured name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for EnvironmentVariableName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        if name.is_empty() {
            return Err(de::Error::custom(
                "environment variable name cannot be empty",
            ));
        }
        if name.contains(['=', '\0']) {
            return Err(de::Error::custom(format!(
                "environment variable name `{name}` cannot contain `=` or NUL"
            )));
        }
        let uppercase = name.to_ascii_uppercase();
        if uppercase == "KARVA" || uppercase.starts_with("KARVA_") {
            return Err(de::Error::custom(format!(
                "environment variable `{name}` is reserved by Karva"
            )));
        }
        Ok(Self(name))
    }
}

/// Operation applied to one worker environment variable before Python starts.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum EnvironmentVariable {
    /// Always set the configured value.
    Set(String),

    /// Set the configured value only when the invoking environment omits it.
    Preserve(String),

    /// Remove the variable from the worker environment.
    Unset,
}

impl Serialize for EnvironmentVariable {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Set(value) => serializer.serialize_str(value),
            Self::Preserve(value) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("value", value)?;
                map.serialize_entry("preserve", &true)?;
                map.end()
            }
            Self::Unset => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("unset", &true)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for EnvironmentVariable {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct EnvironmentVariableVisitor;

        impl<'de> Visitor<'de> for EnvironmentVariableVisitor {
            type Value = EnvironmentVariable;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(
                    "a string, `{ value = \"...\", preserve = true }`, or `{ unset = true }`",
                )
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                validate_value(value)?;
                Ok(EnvironmentVariable::Set(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                validate_value(&value)?;
                Ok(EnvironmentVariable::Set(value))
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut value = None;
                let mut preserve = None;
                let mut unset = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "value" => value = Some(map.next_value::<String>()?),
                        "preserve" => preserve = Some(map.next_value::<bool>()?),
                        "unset" => unset = Some(map.next_value::<bool>()?),
                        _ => {
                            return Err(de::Error::unknown_field(
                                &key,
                                &["value", "preserve", "unset"],
                            ));
                        }
                    }
                }

                match (value, preserve, unset) {
                    (Some(value), Some(true), None) => {
                        validate_value(&value)?;
                        Ok(EnvironmentVariable::Preserve(value))
                    }
                    (None, None, Some(true)) => Ok(EnvironmentVariable::Unset),
                    _ => Err(de::Error::custom(
                        "expected exactly `{ value = \"...\", preserve = true }` or `{ unset = true }`",
                    )),
                }
            }
        }

        deserializer.deserialize_any(EnvironmentVariableVisitor)
    }
}

fn validate_value<E: de::Error>(value: &str) -> Result<(), E> {
    if value.contains('\0') {
        return Err(E::custom("environment variable value cannot contain NUL"));
    }
    Ok(())
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for EnvironmentVariable {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "EnvironmentVariable".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let string = generator.subschema_for::<String>();
        schemars::json_schema!({
            "oneOf": [
                string,
                {
                    "type": "object",
                    "properties": {
                        "value": { "type": "string" },
                        "preserve": { "const": true }
                    },
                    "required": ["value", "preserve"],
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "properties": { "unset": { "const": true } },
                    "required": ["unset"],
                    "additionalProperties": false
                }
            ]
        })
    }
}
