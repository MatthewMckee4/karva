use std::{collections::HashMap, sync::Arc};

use camino::Utf8PathBuf;

use crate::{extensions::fixtures::NormalizedFixture, normalize::models::NormalizedModule};

#[derive(Debug)]
pub struct NormalizedPackage {
    pub(crate) modules: HashMap<Utf8PathBuf, NormalizedModule>,

    pub(crate) packages: HashMap<Utf8PathBuf, Self>,

    pub(crate) auto_use_fixtures: Arc<Vec<NormalizedFixture>>,
}

impl NormalizedPackage {
    pub(crate) fn extend_auto_use_fixtures(&mut self, fixtures: Vec<NormalizedFixture>) {
        let mut combined = (*self.auto_use_fixtures).clone();
        combined.extend(fixtures);
        self.auto_use_fixtures = Arc::new(combined);
    }
}
