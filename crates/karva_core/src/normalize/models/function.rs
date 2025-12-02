use std::{collections::HashMap, sync::Arc};

use camino::Utf8PathBuf;
use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::{
    QualifiedFunctionName,
    extensions::{fixtures::NormalizedFixture, tags::Tags},
};

/// A normalized test represents a concrete test function with resolved dependencies.
///
/// Resolved dependencies include:
/// - params from `tags.parametrize`.
/// - fixtures from function arguments.
/// - use fixtures from `use_fixtures` tag.
/// - auto use fixtures from externally defined autouse fixtures.
#[derive(Debug)]
pub struct NormalizedTest {
    /// Original test function name: "`test_foo`"
    pub(crate) name: QualifiedFunctionName,

    /// Test-level parameters (from @pytest.mark.parametrize)
    /// Maps parameter name to its value for this variant
    pub(crate) params: HashMap<String, Py<PyAny>>,

    /// Normalized fixture dependencies (already expanded)
    /// Each fixture dependency is a specific variant
    /// These are the regular fixtures that should be passed as arguments to the test function
    pub(crate) fixture_dependencies: Arc<Vec<NormalizedFixture>>,

    /// Fixtures from `use_fixtures` tag that should only be executed for side effects
    /// These should NOT be passed as arguments to the test function
    pub(crate) use_fixture_dependencies: Arc<Vec<NormalizedFixture>>,

    /// Fixtures that will be automatically run before the test is executed
    pub(crate) auto_use_fixtures: Arc<Vec<NormalizedFixture>>,

    /// Imported Python function
    pub(crate) function: Py<PyAny>,

    /// Resolved tags
    pub(crate) tags: Tags,

    /// The function definition for this fixture
    pub(crate) stmt_function_def: Arc<StmtFunctionDef>,
}

impl NormalizedTest {
    pub(crate) const fn module_path(&self) -> &Utf8PathBuf {
        self.name.module_path().path()
    }

    pub(crate) fn resolved_tags(&self) -> Tags {
        let mut tags = self.tags.clone();

        for dependency in self.fixture_dependencies.iter() {
            tags.extend(&dependency.resolved_tags());
        }

        for dependency in self.use_fixture_dependencies.iter() {
            tags.extend(&dependency.resolved_tags());
        }

        for dependency in self.auto_use_fixtures.iter() {
            tags.extend(&dependency.resolved_tags());
        }
        tags
    }
}
