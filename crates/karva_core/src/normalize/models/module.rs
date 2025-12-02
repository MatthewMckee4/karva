use std::sync::Arc;

use crate::{extensions::fixtures::NormalizedFixture, normalize::models::NormalizedTest};

#[derive(Debug)]
pub struct NormalizedModule {
    pub(crate) test_functions: Vec<NormalizedTest>,

    pub(crate) auto_use_fixtures: Arc<Vec<NormalizedFixture>>,
}
