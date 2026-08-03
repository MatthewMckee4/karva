use std::rc::Rc;

use camino::{Utf8Path, Utf8PathBuf};
use karva_python_semantic::QualifiedFunctionName;
use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::discovery::models::definition::FunctionDefinition;
use crate::extensions::fixtures::FixtureScope;
use crate::extensions::tags::parametrize::Parametrization;
use crate::runner::FixtureArguments;
use crate::utils::run_coroutine;

/// Stable index into one compiled [`FixturePlan`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FixtureId(usize);

impl FixtureId {
    pub(crate) fn new(index: usize) -> Self {
        Self(index)
    }

    pub(crate) fn index(self) -> usize {
        self.0
    }
}

/// Arena containing every fixture definition needed by one resolution context.
#[derive(Debug)]
pub struct FixturePlan {
    fixtures: Vec<NormalizedFixture>,
    dynamic_fixtures: std::collections::HashMap<String, Vec<FixtureId>>,
    variant_fixture_count: usize,
}

impl FixturePlan {
    pub(crate) fn new(
        fixtures: Vec<NormalizedFixture>,
        dynamic_fixtures: std::collections::HashMap<String, Vec<FixtureId>>,
        variant_fixture_count: usize,
    ) -> Self {
        Self {
            fixtures,
            dynamic_fixtures,
            variant_fixture_count,
        }
    }

    pub(crate) fn fixture(&self, id: FixtureId) -> &NormalizedFixture {
        &self.fixtures[id.0]
    }

    pub(crate) fn dynamic_fixture(&self, name: &str) -> Option<FixtureId> {
        self.dynamic_fixtures.get(name)?.first().copied()
    }

    /// Resolves a dynamic name, advancing through an overridden fixture chain.
    pub(crate) fn dynamic_fixture_for(
        &self,
        name: &str,
        requesting_fixture: Option<FixtureId>,
    ) -> Option<FixtureId> {
        let fixtures = self.dynamic_fixtures.get(name)?;
        let Some(requesting_fixture) = requesting_fixture else {
            return fixtures.first().copied();
        };
        if let Some(index) = fixtures
            .iter()
            .position(|fixture| *fixture == requesting_fixture)
        {
            fixtures.get(index + 1).copied()
        } else {
            fixtures.first().copied()
        }
    }

    pub(crate) fn variant_fixtures(&self) -> impl Iterator<Item = (FixtureId, &NormalizedFixture)> {
        self.fixtures[..self.variant_fixture_count]
            .iter()
            .enumerate()
            .map(|(index, fixture)| (FixtureId::new(index), fixture))
    }

    pub(crate) fn fixture_names(&self) -> impl Iterator<Item = &str> {
        self.fixtures[..self.variant_fixture_count]
            .iter()
            .map(NormalizedFixture::function_name)
    }

    pub(crate) fn uses_request(&self) -> bool {
        self.fixtures[..self.variant_fixture_count]
            .iter()
            .any(NormalizedFixture::requests_request)
    }

    pub(crate) fn requires_variant_execution(&self, id: FixtureId) -> bool {
        let fixture = self.fixture(id);
        fixture.parameters().is_some()
            || fixture.requests_request()
            || fixture
                .dependencies()
                .iter()
                .any(|dependency| self.requires_variant_execution(*dependency))
    }
}

/// A normalized fixture represents a concrete instance of a fixture ready for execution.
///
/// All fixtures — both user-defined and framework-provided — share this single
/// representation. Framework fixtures (from `karva._builtins`) are discovered
/// and normalized the same way as user-defined ones.
#[derive(Debug)]
pub struct NormalizedFixture {
    /// Immutable fixture identity, syntax, and source.
    pub(crate) definition: Rc<FunctionDefinition>,

    /// Resolved fixture dependencies this fixture requires.
    pub(crate) dependencies: Vec<FixtureId>,

    /// The scope at which this fixture's value is cached.
    pub(crate) scope: FixtureScope,

    /// Package whose lifetime owns package-scoped values and finalizers.
    pub(crate) package_owner: Utf8PathBuf,

    /// Whether this fixture uses yield for teardown logic.
    pub(crate) is_generator: bool,

    /// Reference to the Python callable that produces the fixture value.
    pub(crate) py_function: Py<PyAny>,

    /// Parameter values declared by the fixture decorator.
    pub(crate) parameters: Option<Vec<Parametrization>>,

    /// Whether this fixture's cache identity depends on fixture parameters.
    pub(crate) is_parameterized: bool,
}

impl NormalizedFixture {
    /// Returns the fixture's unqualified function name.
    pub(crate) fn function_name(&self) -> &str {
        self.definition.name().function_name()
    }

    pub(crate) fn name(&self) -> &QualifiedFunctionName {
        self.definition.name()
    }

    pub(crate) fn statement(&self) -> &StmtFunctionDef {
        self.definition.statement()
    }

    /// Returns the fixture dependencies.
    pub(crate) fn dependencies(&self) -> &[FixtureId] {
        &self.dependencies
    }

    /// Returns the fixture scope.
    pub(crate) fn scope(&self) -> FixtureScope {
        self.scope
    }

    pub(crate) fn parameters(&self) -> Option<&[Parametrization]> {
        self.parameters.as_deref()
    }

    pub(crate) fn is_parameterized(&self) -> bool {
        self.is_parameterized
    }

    pub(crate) fn requests_request(&self) -> bool {
        self.statement()
            .parameters
            .iter_non_variadic_params()
            .any(|parameter| parameter.parameter.name.as_str() == "request")
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
