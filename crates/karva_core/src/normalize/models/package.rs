use std::rc::Rc;

use crate::extensions::fixtures::NormalizedFixture;
use crate::normalize::models::NormalizedModule;

/// A normalized package containing executable modules and sub-packages.
///
/// Represents the hierarchical structure of tests ready for execution,
/// with package-scoped auto-use fixtures.
#[derive(Debug)]
pub struct NormalizedPackage {
    /// Normalized modules directly in this package.
    pub(crate) modules: Vec<NormalizedModule>,

    /// Normalized sub-packages within this package.
    pub(crate) packages: Vec<Self>,

    /// Package-scoped fixtures that run automatically.
    pub(crate) auto_use_fixtures: Vec<Rc<NormalizedFixture>>,
}

impl NormalizedPackage {
    pub(crate) fn extend_auto_use_fixtures(&mut self, fixtures: Vec<Rc<NormalizedFixture>>) {
        self.auto_use_fixtures.extend(fixtures);
    }
}
