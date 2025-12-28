use camino::Utf8PathBuf;

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
