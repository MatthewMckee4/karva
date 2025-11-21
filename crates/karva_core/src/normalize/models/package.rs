use std::collections::HashMap;

use camino::Utf8PathBuf;

use crate::{name::ModulePath, normalize::models::NormalizedModule};

#[derive(Debug)]
pub struct NormalizedPackage {
    pub(crate) path: Utf8PathBuf,

    pub(crate) modules: HashMap<Utf8PathBuf, NormalizedModule>,

    pub(crate) packages: HashMap<Utf8PathBuf, NormalizedPackage>,

    pub(crate) configuration_module_path: Option<ModulePath>,
}
