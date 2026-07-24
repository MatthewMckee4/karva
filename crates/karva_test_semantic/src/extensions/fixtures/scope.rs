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
    pub(crate) fn scopes_above(self) -> &'static [Self] {
        use FixtureScope::{Function, Module, Package, Session};

        match self {
            Function => &[Function, Module, Package, Session],
            Module => &[Module, Package, Session],
            Package => &[Package, Session],
            Session => &[Session],
        }
    }

    /// Return whether a fixture with this scope may use `dependency`.
    pub(crate) fn can_use(self, dependency: Self) -> bool {
        self.scopes_above().contains(&dependency)
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Module => "module",
            Self::Package => "package",
            Self::Session => "session",
        }
    }

    pub(crate) fn capitalised_name(self) -> &'static str {
        match self {
            Self::Function => "Function",
            Self::Module => "Module",
            Self::Package => "Package",
            Self::Session => "Session",
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

#[cfg(test)]
mod tests {
    use super::FixtureScope::{Function, Module, Package, Session};

    #[test]
    fn scopes_above_returns_current_scope_through_session() {
        assert_eq!(
            Function.scopes_above(),
            &[Function, Module, Package, Session]
        );
        assert_eq!(Module.scopes_above(), &[Module, Package, Session]);
        assert_eq!(Package.scopes_above(), &[Package, Session]);
        assert_eq!(Session.scopes_above(), &[Session]);
    }

    #[test]
    fn fixtures_can_only_use_same_or_broader_scopes() {
        assert!(Function.can_use(Function));
        assert!(Function.can_use(Module));
        assert!(Function.can_use(Package));
        assert!(Function.can_use(Session));

        assert!(!Module.can_use(Function));
        assert!(Module.can_use(Module));
        assert!(Module.can_use(Package));
        assert!(Module.can_use(Session));

        assert!(!Package.can_use(Function));
        assert!(!Package.can_use(Module));
        assert!(Package.can_use(Package));
        assert!(Package.can_use(Session));

        assert!(!Session.can_use(Function));
        assert!(!Session.can_use(Module));
        assert!(!Session.can_use(Package));
        assert!(Session.can_use(Session));
    }
}
