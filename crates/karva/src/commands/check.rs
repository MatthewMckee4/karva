//! Source-only suite health reporting; this command never imports project code.

use std::collections::BTreeSet;
use std::io::{self, Write};

use anyhow::{Context as _, Result};
use camino::Utf8PathBuf;
use ignore::WalkBuilder;
use karva_cli::{CheckCommand, CheckFormat};
use karva_collector::{CollectionSettings, collect_source};
use karva_diagnostic::{
    Annotation, Diagnostic, DiagnosticFormat, DisplayDiagnosticConfig, Severity, Span,
    render_diagnostic,
};
use karva_ide::source_health;
use karva_metadata::{Options, ProjectMetadata, ProjectOptionsOverrides};
use karva_project::{Project, path::absolute};
use ruff_python_ast::PythonVersion;
use ruff_source_file::SourceFileBuilder;
use serde_json::json;

use crate::{ExitStatus, utils::cwd};

/// Checks the current project's selected source paths without starting Python or workers.
pub fn check(args: CheckCommand) -> Result<ExitStatus> {
    let cwd = cwd().context("current directory is not UTF-8")?;
    let config_file = args
        .config
        .config_file
        .as_ref()
        .map(|path| absolute(path, &cwd));
    let mut metadata = if let Some(path) = &config_file {
        ProjectMetadata::from_config_file(path, &cwd, PythonVersion::latest())?
    } else {
        ProjectMetadata::discover(&cwd, PythonVersion::latest())?
    };
    metadata.apply_overrides(
        &ProjectOptionsOverrides::new(config_file, Options::default())
            .with_profile(args.config.profile),
    )?;
    let project = Project::from_metadata(metadata);
    let roots = if args.paths.is_empty() {
        vec![project.cwd().clone()]
    } else {
        args.paths.iter().map(|path| absolute(path, &cwd)).collect()
    };
    let mut paths = BTreeSet::new();
    for root in roots {
        for entry in WalkBuilder::new(&root)
            .require_git(false)
            .git_global(false)
            .git_ignore(project.settings().src().respect_ignore_files)
            .follow_links(false)
            .build()
        {
            let entry = entry.with_context(|| format!("failed to walk source root {root}"))?;
            if entry.file_type().is_some_and(|kind| kind.is_file())
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "py")
            {
                paths.insert(
                    Utf8PathBuf::from_path_buf(entry.into_path()).map_err(|path| {
                        anyhow::anyhow!("source path is not UTF-8: {}", path.display())
                    })?,
                );
            }
        }
    }
    let settings = CollectionSettings {
        python_version: PythonVersion::latest(),
        test_function_prefix: &project.settings().test().test_function_prefix,
        respect_ignore_files: project.settings().src().respect_ignore_files,
        collect_fixtures: true,
        collect_doctests: false,
    };
    let mut findings = Vec::new();
    let mut stdout = io::stdout().lock();
    for path in paths {
        let source =
            std::fs::read_to_string(&path).with_context(|| format!("failed to read {path}"))?;
        let module = collect_source(
            &path,
            path.parent().unwrap_or(project.cwd()),
            source,
            &settings,
            &[],
        )
        .with_context(|| format!("failed to parse {path}"))?;
        let source_file =
            SourceFileBuilder::new(path.as_str(), module.source_text.as_str()).finish();
        for finding in source_health(&module, settings.test_function_prefix) {
            let relative = path.strip_prefix(&cwd).unwrap_or(&path);
            findings.push(json!({
                "code": finding.code.as_str(), "message": finding.message,
                "path": relative, "byte_range": [u32::from(finding.location.range.start()), u32::from(finding.location.range.end())],
                "related": finding.related.iter().map(|related| json!({
                    "message": related.message, "path": relative,
                    "byte_range": [u32::from(related.location.range.start()), u32::from(related.location.range.end())],
                })).collect::<Vec<_>>()
            }));
            if matches!(args.output_format, CheckFormat::Text) {
                let mut diagnostic =
                    Diagnostic::new(finding.code.as_str(), Severity::Warning, finding.message);
                diagnostic.annotate(Annotation::primary(
                    Span::from(source_file.clone()).with_range(finding.location.range),
                ));
                for related in finding.related {
                    diagnostic.annotate(
                        Annotation::secondary(
                            Span::from(source_file.clone()).with_range(related.location.range),
                        )
                        .message(related.message),
                    );
                }
                writeln!(
                    stdout,
                    "{}",
                    render_diagnostic(
                        &diagnostic,
                        &cwd,
                        DisplayDiagnosticConfig::new(DiagnosticFormat::Full, false)
                    )
                    .rendered()
                )?;
            }
        }
    }
    if matches!(args.output_format, CheckFormat::Json) {
        serde_json::to_writer_pretty(&mut stdout, &findings)?;
        writeln!(stdout)?;
    }
    Ok(if findings.is_empty() {
        ExitStatus::Success
    } else {
        ExitStatus::Failure
    })
}
