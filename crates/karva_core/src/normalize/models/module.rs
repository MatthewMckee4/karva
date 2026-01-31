use std::rc::Rc;

use crate::extensions::fixtures::NormalizedFixture;
use crate::normalize::models::NormalizedTest;

#[derive(Debug)]
pub struct NormalizedModule {
    pub(crate) test_functions: Vec<NormalizedTest>,

    pub(crate) auto_use_fixtures: Vec<Rc<NormalizedFixture>>,
}
