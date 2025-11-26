use std::collections::HashMap;

use camino::Utf8PathBuf;

use crate::{extensions::fixtures::NormalizedFixture, normalize::models::NormalizedModule};

#[derive(Debug)]
pub struct NormalizedPackage {
    pub(crate) modules: HashMap<Utf8PathBuf, NormalizedModule>,

    pub(crate) packages: HashMap<Utf8PathBuf, NormalizedPackage>,

    pub(crate) auto_use_fixtures: Vec<NormalizedFixture>,
}

impl NormalizedPackage {
    pub(crate) fn extend_auto_use_fixtures(&mut self, fixtures: Vec<NormalizedFixture>) {
        self.auto_use_fixtures.extend(fixtures);
    }
}
