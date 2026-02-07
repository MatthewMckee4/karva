use pyo3::prelude::*;

/// A scope for a fixture.
#[derive(Copy, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum FixtureScope {
    #[default]
    Function,
    Module,
    Package,
    Session,
}

impl FixtureScope {
    /// Returns a list of scopes above the current scope.
    pub(crate) fn scopes_above(self) -> Vec<Self> {
        use FixtureScope::{Function, Module, Package, Session};

        match self {
            Function => vec![Function, Module, Package, Session],
            Module => vec![Module, Package, Session],
            Package => vec![Package, Session],
            Session => vec![Session],
        }
    }
}

impl TryFrom<String> for FixtureScope {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "module" => Ok(Self::Module),
            "session" => Ok(Self::Session),
            "package" => Ok(Self::Package),
            "function" => Ok(Self::Function),
            _ => Err(format!("Invalid fixture scope: {s}")),
        }
    }
}

impl std::fmt::Display for FixtureScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Module => write!(f, "module"),
            Self::Session => write!(f, "session"),
            Self::Package => write!(f, "package"),
            Self::Function => write!(f, "function"),
        }
    }
}

/// Resolve a dynamic scope function to a concrete `FixtureScope`
pub fn resolve_dynamic_scope(
    py: Python<'_>,
    scope_fn: &Bound<'_, PyAny>,
    fixture_name: &str,
) -> Result<FixtureScope, String> {
    let kwargs = pyo3::types::PyDict::new(py);
    kwargs
        .set_item("fixture_name", fixture_name)
        .map_err(|e| format!("Failed to set fixture_name: {e}"))?;

    // TODO: Support config
    kwargs
        .set_item("config", py.None())
        .map_err(|e| format!("Failed to set config: {e}"))?;

    let result = scope_fn
        .call((), Some(&kwargs))
        .map_err(|e| format!("Failed to call dynamic scope function: {e}"))?;

    let scope_str = result
        .extract::<String>()
        .map_err(|e| format!("Dynamic scope function must return a string: {e}"))?;

    FixtureScope::try_from(scope_str)
}

pub fn fixture_scope(
    py: Python<'_>,
    scope_obj: &Bound<'_, PyAny>,
    name: &str,
) -> Result<FixtureScope, String> {
    if scope_obj.is_callable() {
        resolve_dynamic_scope(py, scope_obj, name)
    } else if let Ok(scope_str) = scope_obj.extract::<String>() {
        FixtureScope::try_from(scope_str)
    } else {
        Err("Scope must be either a string or a callable".to_string())
    }
}
