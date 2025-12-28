use std::str::FromStr;

use camino::Utf8PathBuf;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::module_name;

/// Represents a fully qualified function name including its module path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QualifiedFunctionName {
    function_name: String,
    module_path: ModulePath,
}

impl QualifiedFunctionName {
    pub const fn new(function_name: String, module_path: ModulePath) -> Self {
        Self {
            function_name,
            module_path,
        }
    }

    pub fn function_name(&self) -> &str {
        &self.function_name
    }

    pub const fn module_path(&self) -> &ModulePath {
        &self.module_path
    }
}

impl std::fmt::Display for QualifiedFunctionName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}::{}",
            self.module_path.module_name(),
            self.function_name
        )
    }
}

impl Serialize for QualifiedFunctionName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for QualifiedFunctionName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        // Split on the last "::" to separate module_path from function_name
        let (module_str, function_name) = s
            .rsplit_once("::")
            .ok_or_else(|| serde::de::Error::custom("Invalid qualified function name format"))?;

        let module_path = module_str.parse().map_err(serde::de::Error::custom)?;

        Ok(Self::new(function_name.to_string(), module_path))
    }
}

impl FromStr for QualifiedFunctionName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Split on the last "::" to separate module_path from function_name
        let (module_str, function_name) = s
            .rsplit_once("::")
            .ok_or_else(|| "Invalid qualified function name format".to_string())?;

        let module_path = module_str.parse()?;

        Ok(Self::new(function_name.to_string(), module_path))
    }
}

/// Represents a fully qualified function name including its module path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QualifiedTestName {
    function_name: QualifiedFunctionName,
    full_name: Option<String>,
}

impl QualifiedTestName {
    pub const fn new(function_name: QualifiedFunctionName, full_name: Option<String>) -> Self {
        Self {
            function_name,
            full_name,
        }
    }

    pub const fn function_name(&self) -> &QualifiedFunctionName {
        &self.function_name
    }

    pub fn full_name(&self) -> Option<&str> {
        self.full_name.as_deref()
    }
}

impl std::fmt::Display for QualifiedTestName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(full_name) = &self.full_name {
            write!(f, "{full_name}")
        } else {
            write!(f, "{}", self.function_name)
        }
    }
}

impl Serialize for QualifiedTestName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.function_name.to_string())
    }
}

impl<'de> Deserialize<'de> for QualifiedTestName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let function_name = s.parse().map_err(serde::de::Error::custom)?;
        Ok(Self::new(function_name, None))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ModulePath {
    path: Utf8PathBuf,
    module_name: String,
}

impl ModulePath {
    pub fn new<P: Into<Utf8PathBuf>>(path: P, cwd: &Utf8PathBuf) -> Option<Self> {
        let path = path.into();
        let module_name = module_name(cwd, path.as_ref())?;
        Some(Self { path, module_name })
    }

    pub fn module_name(&self) -> &str {
        self.module_name.as_str()
    }

    pub const fn path(&self) -> &Utf8PathBuf {
        &self.path
    }
}

impl Serialize for ModulePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.module_name)
    }
}

impl<'de> Deserialize<'de> for ModulePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let module_name = String::deserialize(deserializer)?;

        // Convert module name back to path (e.g., "foo.bar.baz" -> "foo/bar/baz.py")
        let path = module_name.replace('.', "/") + ".py";

        Ok(Self {
            path: path.into(),
            module_name,
        })
    }
}

impl FromStr for ModulePath {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Convert module name to path (e.g., "foo.bar.baz" -> "foo/bar/baz.py")
        let path = s.replace('.', "/") + ".py";

        Ok(Self {
            path: path.into(),
            module_name: s.to_string(),
        })
    }
}
