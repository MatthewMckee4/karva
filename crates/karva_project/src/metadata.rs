use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use ruff_python_ast::PythonVersion;
use thiserror::Error;
use ty_combine::Combine;

use crate::{System, VerbosityLevel};

mod configuration_file;
mod options;
mod pyproject;
mod settings;
mod value;

pub use configuration_file::ConfigurationFile;

pub use self::{
    configuration_file::ConfigurationFileError,
    options::{
        KarvaTomlError, Options, OutputFormat, ProjectOptionsOverrides, SrcOptions,
        TerminalOptions, TestOptions, ToSettingsError,
    },
    pyproject::{PyProject, PyProjectError, ResolveRequiresPythonError},
    settings::{ProjectSettings, TestPrefix},
    value::{RangedValue, ValueSource},
};

#[derive(Default, Debug, Clone)]
pub struct ProjectMetadata {
    pub(crate) root: Utf8PathBuf,

    pub(crate) python_version: PythonVersion,

    /// Raw options
    pub(crate) options: Options,

    pub(crate) verbosity: VerbosityLevel,
}

impl ProjectMetadata {
    /// Creates a project with the given name and root that uses the default options.
    pub fn new(
        root: Utf8PathBuf,
        python_version: PythonVersion,
        verbosity: VerbosityLevel,
    ) -> Self {
        Self {
            root,
            python_version,
            options: Options::default(),
            verbosity,
        }
    }

    pub fn from_config_file(
        path: Utf8PathBuf,
        system: &dyn System,
        python_version: PythonVersion,
        verbosity: VerbosityLevel,
    ) -> Result<Self, ProjectMetadataError> {
        tracing::debug!("Using overridden configuration file at '{path}'");

        let config_file = ConfigurationFile::from_path(path.clone(), system).map_err(|error| {
            ProjectMetadataError::ConfigurationFileError {
                source: Box::new(error),
                path,
            }
        })?;

        let options = config_file.into_options();

        Ok(Self {
            root: system.current_directory().to_path_buf(),
            python_version,
            options,
            verbosity,
        })
    }

    /// Loads a project from a `pyproject.toml` file.
    pub(crate) fn from_pyproject(
        pyproject: PyProject,
        root: Utf8PathBuf,
        python_version: PythonVersion,
        verbosity: VerbosityLevel,
    ) -> Self {
        Self::from_options(
            pyproject
                .tool
                .and_then(|tool| tool.karva)
                .unwrap_or_default(),
            root,
            python_version,
            verbosity,
        )
    }

    /// Loads a project from a set of options with an optional pyproject-project table.
    pub const fn from_options(
        options: Options,
        root: Utf8PathBuf,
        python_version: PythonVersion,
        verbosity: VerbosityLevel,
    ) -> Self {
        Self {
            root,
            python_version,
            options,
            verbosity,
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
    pub fn discover(
        path: &Utf8Path,
        system: &dyn System,
        python_version: PythonVersion,
        verbosity: VerbosityLevel,
    ) -> Result<Self, ProjectMetadataError> {
        tracing::debug!("Searching for a project in '{path}'");

        if !system.is_directory(path) {
            return Err(ProjectMetadataError::NotADirectory(path.to_path_buf()));
        }

        let mut closest_project: Option<Self> = None;

        for project_root in path.ancestors() {
            let pyproject_path = project_root.join("pyproject.toml");

            let pyproject = if let Ok(pyproject_str) = system.read_to_string(&pyproject_path) {
                match PyProject::from_toml_str(
                    &pyproject_str,
                    ValueSource::File(Arc::new(pyproject_path.clone())),
                ) {
                    Ok(pyproject) => Some(pyproject),
                    Err(error) => {
                        return Err(ProjectMetadataError::InvalidPyProject {
                            path: pyproject_path,
                            source: Box::new(error),
                        });
                    }
                }
            } else {
                None
            };

            // A `ty.toml` takes precedence over a `pyproject.toml`.
            let ty_toml_path = project_root.join("ty.toml");
            if let Ok(ty_str) = system.read_to_string(&ty_toml_path) {
                let options = match Options::from_toml_str(
                    &ty_str,
                    ValueSource::File(Arc::new(ty_toml_path.clone())),
                ) {
                    Ok(options) => options,
                    Err(error) => {
                        return Err(ProjectMetadataError::InvalidTyToml {
                            path: ty_toml_path,
                            source: Box::new(error),
                        });
                    }
                };

                if pyproject
                    .as_ref()
                    .is_some_and(|project| project.ty().is_some())
                {
                    // TODO: Consider using a diagnostic here
                    tracing::warn!(
                        "Ignoring the `tool.ty` section in `{pyproject_path}` because `{ty_toml_path}` takes precedence."
                    );
                }

                tracing::debug!("Found project at '{}'", project_root);

                let metadata = Self::from_options(
                    options,
                    project_root.to_path_buf(),
                    python_version,
                    verbosity,
                );

                return Ok(metadata);
            }

            if let Some(pyproject) = pyproject {
                let has_ty_section = pyproject.ty().is_some();
                let metadata = Self::from_pyproject(
                    pyproject,
                    project_root.to_path_buf(),
                    python_version,
                    verbosity,
                );

                if has_ty_section {
                    tracing::debug!("Found project at '{}'", project_root);

                    return Ok(metadata);
                }

                // Not a project itself, keep looking for an enclosing project.
                if closest_project.is_none() {
                    closest_project = Some(metadata);
                }
            }
        }

        // No project found, but maybe a pyproject.toml was found.
        let metadata = if let Some(closest_project) = closest_project {
            tracing::debug!(
                "Project without `tool.ty` section: '{}'",
                closest_project.root()
            );

            closest_project
        } else {
            tracing::debug!(
                "The ancestor directories contain no `pyproject.toml`. Falling back to a virtual project."
            );

            // Create a project with a default configuration
            Self::new(path.to_path_buf(), python_version, verbosity)
        };

        Ok(metadata)
    }

    pub const fn python_version(&self) -> PythonVersion {
        self.python_version
    }

    #[must_use]
    pub const fn with_python_version(mut self, python_version: PythonVersion) -> Self {
        self.python_version = python_version;
        self
    }

    pub const fn root(&self) -> &Utf8PathBuf {
        &self.root
    }

    #[must_use]
    pub fn with_root(mut self, root: Utf8PathBuf) -> Self {
        self.root = root;
        self
    }

    pub fn apply_overrides(&mut self, overrides: &ProjectOptionsOverrides) {
        self.options = overrides.apply_to(std::mem::take(&mut self.options));
    }

    /// Combine the project options with the CLI options where the CLI options take precedence.
    pub fn apply_options(&mut self, options: Options) {
        self.options = options.combine(std::mem::take(&mut self.options));
    }
    pub const fn verbosity(&self) -> VerbosityLevel {
        self.verbosity
    }
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

    #[error("{path} is not a valid `ty.toml`: {source}")]
    InvalidTyToml {
        source: Box<KarvaTomlError>,
        path: Utf8PathBuf,
    },

    #[error("Invalid `requires-python` version specifier (`{path}`): {source}")]
    InvalidRequiresPythonConstraint {
        source: ResolveRequiresPythonError,
        path: Utf8PathBuf,
    },

    #[error("Error loading configuration file at {path}: {source}")]
    ConfigurationFileError {
        source: Box<ConfigurationFileError>,
        path: Utf8PathBuf,
    },
}
