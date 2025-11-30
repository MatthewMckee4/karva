use std::{collections::HashMap, sync::Arc};

use camino::Utf8PathBuf;
use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::{
    QualifiedFunctionName,
    extensions::{fixtures::NormalizedFixture, tags::Tags},
};

/// A normalized test function represents a concrete variant of a test after parametrization.
/// For parametrized tests, each parameter combination gets its own `NormalizedTestFunction`.
#[derive(Debug)]
pub struct NormalizedTestFunction {
    /// Original test function name: "`test_foo`"
    pub(crate) name: QualifiedFunctionName,

    /// Test-level parameters (from @pytest.mark.parametrize)
    /// Maps parameter name to its value for this variant
    pub(crate) params: HashMap<String, Py<PyAny>>,

    /// Normalized fixture dependencies (already expanded)
    /// Each fixture dependency is a specific variant
    /// These are the regular fixtures that should be passed as arguments to the test function
    pub(crate) fixture_dependencies: Vec<NormalizedFixture>,

    /// Fixtures from `use_fixtures` tag that should only be executed for side effects
    /// These should NOT be passed as arguments to the test function
    pub(crate) use_fixture_dependencies: Vec<NormalizedFixture>,

    pub(crate) auto_use_fixtures: Vec<NormalizedFixture>,

    /// Imported Python function
    pub(crate) function: Py<PyAny>,

    /// Resolved tags
    pub(crate) tags: Tags,

    /// The function definition for this fixture
    pub(crate) stmt_function_def: Arc<StmtFunctionDef>,
}

impl NormalizedTestFunction {
    pub(crate) const fn module_path(&self) -> &Utf8PathBuf {
        self.name.module_path().path()
    }
}
