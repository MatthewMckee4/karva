use std::rc::Rc;

use crate::extensions::fixtures::NormalizedFixture;
use crate::normalize::models::NormalizedModule;

#[derive(Debug)]
pub struct NormalizedPackage {
    pub(crate) modules: Vec<NormalizedModule>,

    pub(crate) packages: Vec<Self>,

    pub(crate) auto_use_fixtures: Vec<Rc<NormalizedFixture>>,
}

impl NormalizedPackage {
    pub(crate) fn extend_auto_use_fixtures(&mut self, fixtures: Vec<Rc<NormalizedFixture>>) {
        self.auto_use_fixtures.extend(fixtures);
    }
}
