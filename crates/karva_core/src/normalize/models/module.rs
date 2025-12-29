use std::sync::Arc;

use karva_python_semantic::ModulePath;

use crate::extensions::fixtures::NormalizedFixture;
use crate::normalize::models::NormalizedTest;

#[derive(Debug)]
pub struct NormalizedModule {
    pub(crate) path: ModulePath,

    pub(crate) test_functions: Vec<NormalizedTest>,

    pub(crate) auto_use_fixtures: Vec<Arc<NormalizedFixture>>,
}
