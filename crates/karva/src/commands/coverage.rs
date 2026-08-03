use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};

use anyhow::{Context, Result, bail};
use karva_cli::{CoverageAction, CoverageCommand, CoverageFormat, CoverageSort};
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

    let data_files = coverage_data_files(&data_file)?;

    let filters = karva_coverage::CoverageFilters::new(&settings.include, &settings.omit)?
        .with_contexts(&settings.contexts)?
        .with_path_aliases(&settings.path_aliases)?;
    let analysis =
        karva_coverage::CoverageAnalysis::load_native(project.cwd(), &data_files, &filters)?
            .with_context(|| format!("coverage data at `{data_file}` contains no source files"))?;

    let total = match &args.action {
        CoverageAction::Report(report) => {
            if report.append == Some(true) && report.format != CoverageFormat::Markdown {
                bail!("`--append` requires `--format markdown`");
            }
            let options = karva_coverage::CoverageReportOptions {
                selectors: report.selectors.clone(),
                show_missing: report.show_missing,
                skip_covered: report.skip_covered,
                skip_empty: report.skip_empty,
                sort: match report.sort {
                    CoverageSort::Name => karva_coverage::CoverageReportSort::Name,
                    CoverageSort::Statements => karva_coverage::CoverageReportSort::Statements,
                    CoverageSort::Misses => karva_coverage::CoverageReportSort::Misses,
                    CoverageSort::Branches => karva_coverage::CoverageReportSort::Branches,
                    CoverageSort::PartialBranches => {
                        karva_coverage::CoverageReportSort::PartialBranches
                    }
                    CoverageSort::Coverage => karva_coverage::CoverageReportSort::Coverage,
                },
                precision: settings.precision.0,
                format: match report.format {
                    CoverageFormat::Text => karva_coverage::CoverageReportFormat::Text,
                    CoverageFormat::Markdown => karva_coverage::CoverageReportFormat::Markdown,
                    CoverageFormat::Total => karva_coverage::CoverageReportFormat::Total,
                },
            };
            if let Some(output) = &report.output {
                let output = absolute(output, project.cwd());
                if let Some(parent) = output.parent()
                    && !parent.as_str().is_empty()
                {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create coverage report directory `{parent}`")
                    })?;
                }
                let mut file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .append(report.append == Some(true))
                    .truncate(report.append != Some(true))
                    .open(&output)
                    .with_context(|| format!("failed to open coverage report `{output}`"))?;
                analysis.write_report(&options, &mut file)?
            } else {
                analysis.write_report(&options, &mut std::io::stdout().lock())?
            }
        }
        CoverageAction::Html(html) => {
            let output = absolute(&html.directory, project.cwd());
            analysis.write_html_with_options(
                &output,
                &karva_coverage::HtmlReportOptions {
                    title: html.title.clone(),
                    show_contexts: html.show_contexts,
                    skip_covered: html.skip_covered,
                    skip_empty: html.skip_empty,
                    precision: settings.precision.0,
                },
            )?
        }
    };
    if let Some(threshold) = settings.fail_under
        && total < threshold
    {
        let mut stderr = std::io::stderr().lock();
        writeln!(
            stderr,
            "coverage failure: required total coverage of {threshold}% not reached, total coverage was {:.*}%",
            settings.precision.0, total
        )?;
        return Ok(ExitStatus::Error);
    }

    Ok(ExitStatus::Success)
}

fn coverage_data_files(data_file: &camino::Utf8Path) -> Result<Vec<camino::Utf8PathBuf>> {
    let mut files = Vec::new();
    match std::fs::metadata(data_file) {
        Ok(metadata) if metadata.is_file() => files.push(data_file.to_path_buf()),
        Ok(_) => bail!("coverage data path `{data_file}` is not a file"),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect coverage data `{data_file}`"));
        }
    }

    let pending = data_file
        .parent()
        .unwrap_or_else(|| camino::Utf8Path::new("."))
        .join("pending");
    match std::fs::read_dir(&pending) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.with_context(|| {
                    format!("failed to read pending coverage directory `{pending}`")
                })?;
                if !entry
                    .file_type()
                    .with_context(|| format!("failed to inspect `{}`", entry.path().display()))?
                    .is_file()
                {
                    continue;
                }
                let path = camino::Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| {
                    anyhow::anyhow!(
                        "pending coverage path contains non-Unicode characters: `{}`",
                        path.display()
                    )
                })?;
                if path.extension() == Some("json") {
                    files.push(path);
                }
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read pending coverage directory `{pending}`"));
        }
    }
    files.sort();
    if files.is_empty() {
        bail!("no coverage data found at `{data_file}`; run `uv run karva test --cov` first");
    }
    Ok(files)
}
