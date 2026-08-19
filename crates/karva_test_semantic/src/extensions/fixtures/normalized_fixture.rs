use std::rc::Rc;

use camino::{Utf8Path, Utf8PathBuf};
use karva_python_semantic::QualifiedFunctionName;
use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::discovery::models::definition::FunctionDefinition;
use crate::extensions::fixtures::{FixtureIdentity, FixtureScope};
use crate::runner::FixtureArguments;
use crate::utils::run_coroutine;

/// Stable index into one compiled [`FixturePlan`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FixtureId(usize);

impl FixtureId {
    pub(crate) fn new(index: usize) -> Self {
        Self(index)
    }
}

/// Arena containing every fixture definition needed by one resolution context.
#[derive(Debug)]
pub struct FixturePlan {
    fixtures: Vec<NormalizedFixture>,
}

impl FixturePlan {
    pub(crate) fn new(fixtures: Vec<NormalizedFixture>) -> Self {
        Self { fixtures }
    }

    pub(crate) fn fixture(&self, id: FixtureId) -> &NormalizedFixture {
        &self.fixtures[id.0]
    }
}

/// A normalized fixture represents a concrete instance of a fixture ready for execution.
///
/// All fixtures — both user-defined and framework-provided — share this single
/// representation. Framework fixtures (from `karva._builtins`) are discovered
/// and normalized the same way as user-defined ones.
#[derive(Debug)]
pub struct NormalizedFixture {
    /// Qualified identity and immutable source definition.
    pub(crate) identity: FixtureIdentity,

    /// Resolved fixture dependencies this fixture requires.
    pub(crate) dependencies: Vec<FixtureId>,

    /// The scope at which this fixture's value is cached.
    pub(crate) scope: FixtureScope,

    /// Package whose lifetime owns package-scoped values and finalizers; empty for other scopes.
    pub(crate) package_owner: Utf8PathBuf,

    /// Whether this fixture uses yield for teardown logic.
    pub(crate) is_generator: bool,

    /// Reference to the Python callable that produces the fixture value.
    pub(crate) py_function: Py<PyAny>,
}

impl NormalizedFixture {
    /// Returns the fixture's unqualified function name.
    pub(crate) fn function_name(&self) -> &str {
        self.identity.name().function_name()
    }

    /// Returns the stable qualified fixture identity.
    pub(crate) fn name(&self) -> &QualifiedFunctionName {
        self.identity.name()
    }

    pub(crate) fn identity(&self) -> &FixtureIdentity {
        &self.identity
    }

    pub(crate) fn definition(&self) -> &Rc<FunctionDefinition> {
        self.identity.definition()
    }

    pub(crate) fn statement(&self) -> &StmtFunctionDef {
        self.identity.definition().statement()
    }

    /// Returns the fixture dependencies.
    pub(crate) fn dependencies(&self) -> &[FixtureId] {
        &self.dependencies
    }

    /// Returns the fixture scope.
    pub(crate) fn scope(&self) -> FixtureScope {
        self.scope
    }

    /// Returns the package that owns this fixture's package-scoped state.
    pub(crate) fn package_owner(&self) -> &Utf8Path {
        &self.package_owner
    }

    /// Call this fixture with the already-resolved arguments and return the result.
    pub(crate) fn call(
        &self,
        py: Python,
        fixture_arguments: &FixtureArguments,
    ) -> PyResult<Py<PyAny>> {
        let result = if fixture_arguments.is_empty() {
            self.py_function.call0(py)
        } else {
            let kwargs_dict = fixture_arguments.to_kwargs(py)?;
            self.py_function.call(py, (), Some(&kwargs_dict))
        };

        if self.statement().is_async && !self.is_generator {
            result.and_then(|coroutine| run_coroutine(py, coroutine))
        } else {
            result
        }
    }
}
