use std::collections::HashMap;
use std::sync::Arc;

use camino::Utf8PathBuf;

use crate::extensions::fixtures::NormalizedFixture;
use crate::normalize::models::NormalizedModule;

#[derive(Debug)]
pub struct NormalizedPackage {
    pub(crate) path: Utf8PathBuf,

    pub(crate) modules: HashMap<Utf8PathBuf, NormalizedModule>,

    pub(crate) packages: HashMap<Utf8PathBuf, Self>,

    pub(crate) auto_use_fixtures: Vec<Arc<NormalizedFixture>>,
}

impl NormalizedPackage {
    pub(crate) fn extend_auto_use_fixtures(&mut self, fixtures: Vec<Arc<NormalizedFixture>>) {
        self.auto_use_fixtures.extend(fixtures);
    }
}
