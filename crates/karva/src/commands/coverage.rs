use std::io::{ErrorKind, Write};

use anyhow::{Context, Result, bail};
use karva_cli::{CoverageAction, CoverageCommand};
use karva_logging::Printer;
use karva_metadata::{ProjectMetadata, ProjectOptionsOverrides};
use karva_project::Project;
use karva_project::path::absolute;
use karva_python_semantic::current_python_version;

use crate::ExitStatus;
use crate::utils::cwd;

pub fn coverage(args: &CoverageCommand) -> Result<ExitStatus> {
    let cwd = cwd()?;
    let config_file = args.config_file.as_ref().map(|path| absolute(path, &cwd));
    let mut metadata = if let Some(config_file) = &config_file {
        ProjectMetadata::from_config_file(config_file, &cwd, current_python_version())?
    } else {
        ProjectMetadata::discover(&cwd, current_python_version())?
    };
    let overrides = ProjectOptionsOverrides::new(config_file, args.options())
        .with_profile(args.profile.clone());
    metadata
        .apply_overrides(&overrides)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let project = Project::from_metadata(metadata);
    let settings = project.settings().coverage();
    let data_file = absolute(&settings.data_file, project.cwd());

    match std::fs::metadata(&data_file) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {
            bail!("no coverage data found at `{data_file}`; run `uv run karva test --cov` first");
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect coverage data `{data_file}`"));
        }
    }

    let filters = karva_coverage::CoverageFilters::new(&settings.include, &settings.omit)?
        .with_contexts(&settings.contexts)?;
    let analysis = karva_coverage::CoverageAnalysis::load_native(
        project.cwd(),
        std::slice::from_ref(&data_file),
        &filters,
    )?
    .with_context(|| format!("coverage data at `{data_file}` contains no source files"))?;

    match &args.action {
        CoverageAction::Report(_) => {
            let total = analysis.report_with_precision(false, settings.precision.0)?;
            if let Some(threshold) = settings.fail_under
                && total < threshold
            {
                let mut stdout = Printer::default().stream_for_message().lock();
                writeln!(
                    stdout,
                    "\ncoverage failure: required total coverage of {threshold}% not reached, total coverage was {:.*}%",
                    settings.precision.0, total
                )?;
                return Ok(ExitStatus::Failure);
            }
        }
    }

    Ok(ExitStatus::Success)
}
