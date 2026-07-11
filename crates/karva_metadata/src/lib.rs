use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;
use karva_combine::Combine;
use ruff_python_ast::PythonVersion;
use thiserror::Error;

pub mod filter;
mod max_fail;
mod options;
mod pyproject;
mod settings;

pub use max_fail::MaxFail;
pub use options::{
    Config, CovReport, CoverageOptions, DEFAULT_PROFILE, IncompatibleVersionError, JunitOptions,
    Options, OutputFormat, OverrideOptions, ProjectOptionsOverrides, SrcOptions, TerminalOptions,
    TestOptions, UnknownProfile,
};
pub use pyproject::{PyProject, PyProjectError};
pub use settings::{
    CovFailUnder, CoverageSettings, JunitSettings, NoTestsMode, OverrideSettings, ProjectSettings,
    RunIgnoredMode, RunTimeoutSecs, SlowTimeoutSecs, TerminationGracePeriodSecs, TestTimeoutSecs,
};

use crate::options::KarvaTomlError;

/// File-level configuration paired with the resolved per-profile [`Options`].
///
/// `config` always reflects the file as parsed. `options` is empty until
/// [`ProjectMetadata::apply_overrides`] selects a profile and combines CLI
/// overrides on top of it.
#[derive(Default, Debug, Clone)]
pub struct ProjectMetadata {
    pub root: Utf8PathBuf,

    pub python_version: PythonVersion,

    pub config: Config,

    pub options: Options,
}

impl ProjectMetadata {
    /// Creates a project with the given root and an empty configuration.
    pub fn new(root: Utf8PathBuf, python_version: PythonVersion) -> Self {
        Self {
            root,
            python_version,
            config: Config::default(),
            options: Options::default(),
        }
    }

    pub fn from_config_file(
        path: &Utf8Path,
        cwd: &Utf8Path,
        python_version: PythonVersion,
    ) -> Result<Self, ProjectMetadataError> {
        tracing::debug!("Using overridden configuration file at '{path}'");

        let config = Config::from_karva_configuration_file(path).map_err(|error| {
            ProjectMetadataError::InvalidKarvaToml {
                source: Box::new(error),
                path: path.to_path_buf(),
            }
        })?;

        check_required_version(&config, path)?;

        Ok(Self {
            root: cwd.to_path_buf(),
            python_version,
            config,
            options: Options::default(),
        })
    }

    /// Loads a project from a `pyproject.toml` file.
    pub(crate) fn from_pyproject(
        pyproject: PyProject,
        root: Utf8PathBuf,
        python_version: PythonVersion,
    ) -> Self {
        Self::from_config(
            pyproject
                .tool
                .and_then(|tool| tool.karva)
                .unwrap_or_default(),
            root,
            python_version,
        )
    }

    /// Loads a project from a parsed [`Config`].
    pub fn from_config(config: Config, root: Utf8PathBuf, python_version: PythonVersion) -> Self {
        Self {
            root,
            python_version,
            config,
            options: Options::default(),
        }
    }

    /// Discovers the closest project at `path` and returns its metadata.
    ///
    /// The algorithm traverses upwards in the `path`'s ancestor chain and uses the following precedence
    /// the resolve the project's root.
    ///
    /// 1. The closest `pyproject.toml` with a `tool.karva` section or `karva.toml`.
    /// 1. The closest `pyproject.toml`.
    /// 1. Fallback to use `path` as the root and use the default settings.
    ///
    /// The upward walk stops at the first directory that contains a `.git` entry.
    /// This prevents karva from escaping the current repository and accidentally
    /// picking up a parent project's configuration (e.g., in monorepos or when
    /// a project is cloned inside another repo's working tree).
    pub fn discover(
        path: &Utf8Path,
        python_version: PythonVersion,
    ) -> Result<Self, ProjectMetadataError> {
        tracing::debug!("Searching for a project in '{path}'");

        if !path.as_std_path().is_dir() {
            return Err(ProjectMetadataError::NotADirectory(path.to_path_buf()));
        }

        let mut closest_project: Option<Self> = None;

        for project_root in path.ancestors() {
            let pyproject = try_load_pyproject(project_root)?;

            if let Some(config) = try_load_karva_toml(project_root)? {
                if has_karva_section(pyproject.as_ref()) {
                    let pyproject_path = project_root.join("pyproject.toml");
                    let karva_toml_path = project_root.join("karva.toml");
                    tracing::warn!(
                        "Ignoring the `tool.karva` section in `{pyproject_path}` because `{karva_toml_path}` takes precedence."
                    );
                }

                check_required_version(&config, &project_root.join("karva.toml"))?;

                tracing::debug!("Found project at '{}'", project_root);
                return Ok(Self::from_config(
                    config,
                    project_root.to_path_buf(),
                    python_version,
                ));
            }

            if let Some(pyproject) = pyproject {
                let has_karva = pyproject.karva().is_some();
                if let Some(config) = pyproject.karva() {
                    check_required_version(config, &project_root.join("pyproject.toml"))?;
                }
                let metadata =
                    Self::from_pyproject(pyproject, project_root.to_path_buf(), python_version);

                if has_karva {
                    tracing::debug!("Found project at '{}'", project_root);
                    return Ok(metadata);
                }

                if closest_project.is_none() {
                    closest_project = Some(metadata);
                }
            }

            // Stop walking up at a `.git` boundary to avoid escaping the current
            // repository and picking up a parent project's configuration.
            if project_root.join(".git").exists() {
                tracing::debug!(
                    "Stopping project discovery at git boundary '{}'",
                    project_root
                );
                break;
            }
        }

        if let Some(closest_project) = closest_project {
            tracing::debug!(
                "Project without `tool.karva` section: '{}'",
                closest_project.root()
            );
            Ok(closest_project)
        } else {
            tracing::debug!(
                "The ancestor directories contain no `pyproject.toml`. Falling back to a virtual project."
            );
            Ok(Self::new(path.to_path_buf(), python_version))
        }
    }

    pub fn python_version(&self) -> PythonVersion {
        self.python_version
    }

    pub fn root(&self) -> &Utf8PathBuf {
        &self.root
    }

    #[must_use]
    pub fn with_root(mut self, root: Utf8PathBuf) -> Self {
        self.root = root;
        self
    }

    /// Resolve the requested profile from the parsed [`Config`] and combine
    /// CLI overrides on top, populating `self.options`.
    pub fn apply_overrides(
        &mut self,
        overrides: &ProjectOptionsOverrides,
    ) -> Result<(), UnknownProfile> {
        let config = std::mem::take(&mut self.config);
        self.options = overrides.apply_to(config)?;
        Ok(())
    }

    /// Combine the project options with the CLI options where the CLI options take precedence.
    pub fn apply_options(&mut self, options: Options) {
        self.options = options.combine(std::mem::take(&mut self.options));
    }
}

/// Checks for a `karva.toml` in `dir` and parses it if present.
fn try_load_karva_toml(dir: &Utf8Path) -> Result<Option<Config>, ProjectMetadataError> {
    let path = dir.join("karva.toml");

    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ProjectMetadataError::InvalidKarvaToml {
                source: Box::new(KarvaTomlError::FileReadError {
                    source,
                    path: path.clone(),
                }),
                path,
            });
        }
    };

    let config = Config::from_toml_str(&content).map_err(|error| {
        ProjectMetadataError::InvalidKarvaToml {
            source: Box::new(error),
            path,
        }
    })?;

    Ok(Some(config))
}

/// Checks for a `pyproject.toml` in `dir` and parses it if present.
fn try_load_pyproject(dir: &Utf8Path) -> Result<Option<PyProject>, ProjectMetadataError> {
    let path = dir.join("pyproject.toml");

    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ProjectMetadataError::InvalidPyProject {
                path: path.clone(),
                source: Box::new(PyProjectError::FileReadError { source, path }),
            });
        }
    };

    let pyproject = PyProject::from_toml_str(&content).map_err(|error| {
        ProjectMetadataError::InvalidPyProject {
            path,
            source: Box::new(error),
        }
    })?;

    Ok(Some(pyproject))
}

#[cfg(test)]
mod tests {
    use camino::Utf8Path;

    use super::*;

    fn temp_path(dir: &tempfile::TempDir) -> &Utf8Path {
        Utf8Path::from_path(dir.path()).expect("temp path should be UTF-8")
    }

    #[test]
    fn karva_toml_read_errors_are_reported() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let root = temp_path(&temp_dir);
        std::fs::create_dir(root.join("karva.toml")).expect("create directory config path");

        let err = try_load_karva_toml(root).expect_err("directory config path should fail");

        assert!(matches!(
            err,
            ProjectMetadataError::InvalidKarvaToml { ref source, .. }
                if matches!(source.as_ref(), KarvaTomlError::FileReadError { .. })
        ));
    }

    #[test]
    fn pyproject_read_errors_are_reported() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let root = temp_path(&temp_dir);
        std::fs::create_dir(root.join("pyproject.toml")).expect("create directory config path");

        let err = try_load_pyproject(root).expect_err("directory config path should fail");

        assert!(matches!(
            err,
            ProjectMetadataError::InvalidPyProject { ref source, .. }
                if matches!(source.as_ref(), PyProjectError::FileReadError { .. })
        ));
    }
}

/// Returns `true` if the pyproject contains a `[tool.karva]` section.
fn has_karva_section(pyproject: Option<&PyProject>) -> bool {
    pyproject.is_some_and(|project| project.karva().is_some())
}

fn check_required_version(config: &Config, path: &Utf8Path) -> Result<(), ProjectMetadataError> {
    config
        .check_required_version(karva_version::version())
        .map_err(|source| ProjectMetadataError::IncompatibleVersion {
            path: path.to_path_buf(),
            source,
        })
}

#[derive(Debug, Error)]
pub enum ProjectMetadataError {
    #[error("project path '{0}' is not a directory")]
    NotADirectory(Utf8PathBuf),

    #[error("{path} is not a valid `pyproject.toml`: {source}")]
    InvalidPyProject {
        source: Box<PyProjectError>,
        path: Utf8PathBuf,
    },

    #[error("{path} is not a valid `karva.toml`: {source}")]
    InvalidKarvaToml {
        source: Box<KarvaTomlError>,
        path: Utf8PathBuf,
    },

    #[error("{path} has an incompatible `required-version`")]
    IncompatibleVersion {
        path: Utf8PathBuf,
        #[source]
        source: IncompatibleVersionError,
    },
}
