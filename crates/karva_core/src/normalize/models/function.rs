use std::collections::HashMap;
use std::rc::Rc;

use camino::Utf8PathBuf;
use karva_python_semantic::QualifiedFunctionName;
use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::extensions::fixtures::NormalizedFixture;
use crate::extensions::tags::Tags;

/// A concrete test instance ready for execution.
///
/// Represents a single test invocation with all dependencies resolved:
/// parametrize values expanded, fixtures identified, and tags combined.
#[derive(Debug)]
pub struct NormalizedTest {
    /// Fully qualified name of the test function.
    pub(crate) name: QualifiedFunctionName,

    /// Parameter values for this test variant (from @parametrize).
    pub(crate) params: HashMap<String, Py<PyAny>>,

    /// Fixtures to be passed as arguments to the test function.
    pub(crate) fixture_dependencies: Vec<Rc<NormalizedFixture>>,

    /// Fixtures from @usefixtures (run for side effects, not passed as args).
    pub(crate) use_fixture_dependencies: Vec<Rc<NormalizedFixture>>,

    /// Auto-use fixtures that run automatically before this test.
    pub(crate) auto_use_fixtures: Vec<Rc<NormalizedFixture>>,

    /// Reference to the Python callable to execute.
    pub(crate) function: Py<PyAny>,

    /// Combined tags from the test and its parameter set.
    pub(crate) tags: Tags,

    /// AST representation for diagnostic reporting.
    pub(crate) stmt_function_def: Rc<StmtFunctionDef>,
}

impl NormalizedTest {
    pub(crate) const fn module_path(&self) -> &Utf8PathBuf {
        self.name.module_path().path()
    }

    pub(crate) fn resolved_tags(&self) -> Tags {
        let mut tags = self.tags.clone();

        for dependency in &self.fixture_dependencies {
            tags.extend(&dependency.resolved_tags());
        }

        for dependency in &self.use_fixture_dependencies {
            tags.extend(&dependency.resolved_tags());
        }

        for dependency in &self.auto_use_fixtures {
            tags.extend(&dependency.resolved_tags());
        }
        tags
    }
}
