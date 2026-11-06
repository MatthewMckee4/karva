//! Imports supported pytest project settings as fallbacks for Karva configuration.

use serde::{Deserialize, Serialize};

use crate::options::{Config, DEFAULT_PROFILE, TestOptions};

#[derive(Deserialize, Serialize, Debug, Default, Clone, PartialEq, Eq)]
/// Supported subset of `[tool.pytest]`; unrelated pytest options are ignored.
pub struct PytestOptions {
    /// Legacy pytest options stored in `pyproject.toml`.
    ini_options: Option<PytestIniOptions>,
}

#[derive(Deserialize, Serialize, Debug, Default, Clone, PartialEq, Eq)]
/// Warning policy from `[tool.pytest.ini_options]`.
struct PytestIniOptions {
    /// Ordered Python warning filters; later matching entries win.
    filterwarnings: Option<Vec<String>>,
}

impl PytestOptions {
    /// Imports warning policy into the default profile without overriding an
    /// explicit Karva list, including an empty list. Normal profile and CLI
    /// resolution can replace the imported policy later.
    pub(super) fn apply_warning_filters(&self, config: &mut Config) {
        if let Some(filters) = self
            .ini_options
            .as_ref()
            .and_then(|options| options.filterwarnings.as_ref())
        {
            let options = config
                .profile
                .entry(DEFAULT_PROFILE.to_owned())
                .or_default();
            let test = options.test.get_or_insert_with(TestOptions::default);
            if test.warning_filters.is_none() {
                test.warning_filters = Some(filters.clone());
            }
        }
    }
}
