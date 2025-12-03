use camino::Utf8PathBuf;
use karva_project::module_name;

/// Represents a fully qualified function name including its module path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QualifiedFunctionName {
    function_name: String,
    module_path: ModulePath,
}

impl QualifiedFunctionName {
    pub(crate) const fn new(function_name: String, module_path: ModulePath) -> Self {
        Self {
            function_name,
            module_path,
        }
    }

    pub(crate) fn function_name(&self) -> &str {
        &self.function_name
    }

    pub(crate) const fn module_path(&self) -> &ModulePath {
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ModulePath {
    path: Utf8PathBuf,
    module_name: String,
}

impl ModulePath {
    pub(crate) fn new<P: Into<Utf8PathBuf>>(path: P, cwd: &Utf8PathBuf) -> Option<Self> {
        let path = path.into();
        let module_name = module_name(cwd, path.as_ref())?;
        Some(Self { path, module_name })
    }

    pub(crate) fn module_name(&self) -> &str {
        self.module_name.as_str()
    }

    pub(crate) const fn path(&self) -> &Utf8PathBuf {
        &self.path
    }
}
