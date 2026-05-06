use std::fmt::Write;

use anyhow::{Context as _, Result};
use karva_cli::ShowConfigCommand;
use karva_logging::Printer;
use karva_metadata::{Options, ProjectMetadata, ProjectOptionsOverrides};
use karva_project::Project;
use karva_project::path::absolute;
use karva_python_semantic::current_python_version;

use crate::ExitStatus;
use crate::utils::cwd;

pub fn show_config(args: ShowConfigCommand) -> Result<ExitStatus> {
    let cwd = cwd().map_err(|_| {
        anyhow::anyhow!(
            "The current working directory contains non-Unicode characters. karva only supports Unicode paths."
        )
    })?;

    let python_version = current_python_version();

    let config_file = args.config_file.as_ref().map(|path| absolute(path, &cwd));

    let mut project_metadata = if let Some(config_file) = &config_file {
        ProjectMetadata::from_config_file(config_file.clone(), &cwd, python_version)?
    } else {
        ProjectMetadata::discover(&cwd, python_version)?
    };

    let overrides =
        ProjectOptionsOverrides::new(config_file, Options::default()).with_profile(args.profile);
    project_metadata
        .apply_overrides(&overrides)
        .map_err(|err| anyhow::anyhow!("{err}"))?;

    let project = Project::from_metadata(project_metadata);
    let resolved = project.settings().to_options();

    let serialized = toml::to_string(&resolved).context("failed to serialize configuration")?;

    let mut stdout = Printer::default().stream_for_message().lock();
    write!(stdout, "{serialized}")?;

    Ok(ExitStatus::Success)
}
