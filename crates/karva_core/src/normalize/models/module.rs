use std::rc::Rc;

use crate::extensions::fixtures::NormalizedFixture;
use crate::normalize::models::NormalizedTest;

/// A normalized module containing executable test instances.
///
/// Groups all normalized tests from a single Python file along with
/// module-scoped auto-use fixtures.
#[derive(Debug)]
pub struct NormalizedModule {
    /// All concrete test instances to execute from this module.
    pub(crate) test_functions: Vec<NormalizedTest>,

    /// Module-scoped fixtures that run automatically.
    pub(crate) auto_use_fixtures: Vec<Rc<NormalizedFixture>>,
}
