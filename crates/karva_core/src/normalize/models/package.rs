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
    pub(crate) const fn new(
        modules: HashMap<Utf8PathBuf, NormalizedModule>,
        packages: HashMap<Utf8PathBuf, Self>,
    ) -> Self {
        Self {
            modules,
            packages,
            auto_use_fixtures: Vec::new(),
        }
    }
}
