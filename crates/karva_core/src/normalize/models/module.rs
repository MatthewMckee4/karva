use crate::{extensions::fixtures::NormalizedFixture, normalize::models::NormalizedTestFunction};

#[derive(Debug)]
pub struct NormalizedModule {
    pub(crate) test_functions: Vec<NormalizedTestFunction>,

    pub(crate) auto_use_fixtures: Vec<NormalizedFixture>,
}
