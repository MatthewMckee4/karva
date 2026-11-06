//! Source inspection shares project resolution and collection with the runner.
//! Python is never imported here; dynamic behavior remains unknown.

use std::io::{self, Write};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use camino::Utf8Path;
use karva_cli::{HealthCommand, HealthFailOn, HealthFormat};
use karva_collector::{CollectedModule, CollectionSettings, collect_source_with_module_name};
use karva_ide::{DiagnosticCode, SourceAnalysisSettings, SourceDiagnostic, WorkspaceSourceIndex};
use karva_metadata::{Options, ProjectMetadata, ProjectOptionsOverrides, SrcOptions};
use karva_project::Project;
use karva_project::path::absolute;
use ruff_python_ast::PythonVersion;
use ruff_text_size::TextRange;
use serde::Serialize;

use crate::ExitStatus;
use crate::utils::cwd;

/// Versioned, deterministic machine interface for suite inspection.
#[derive(Serialize)]
struct HealthReport {
    schema_version: u8,
    analysis: &'static str,
    findings: Vec<Finding>,
}

/// A finding retains evidence separately from the action proposed to the user.
#[derive(Serialize)]
struct Finding {
    rule: &'static str,
    severity: &'static str,
    location: Location,
    evidence: String,
    related: Vec<Related>,
    action: &'static str,
}

#[derive(Serialize)]
struct Related {
    location: Location,
    evidence: String,
}

/// Lines and Unicode columns are one-based; byte offsets are zero-based UTF-8.
#[derive(Serialize)]
struct Location {
    path: String,
    line: usize,
    column: usize,
    start: u32,
    end: u32,
}

impl Location {
    fn new(
        path: &Utf8Path,
        range: TextRange,
        modules: &[Arc<CollectedModule>],
        root: &Utf8Path,
    ) -> Self {
        let before = modules
            .iter()
            .find(|module| module.path.path() == path)
            .and_then(|module| module.source_text.get(..usize::from(range.start())))
            .unwrap_or_default();
        Self {
            path: path
                .strip_prefix(root)
                .unwrap_or(path)
                .as_str()
                .replace('\\', "/"),
            line: before.bytes().filter(|byte| *byte == b'\n').count() + 1,
            column: before
                .rsplit('\n')
                .next()
                .unwrap_or_default()
                .chars()
                .count()
                + 1,
            start: range.start().to_u32(),
            end: range.end().to_u32(),
        }
    }
}

pub fn health(args: HealthCommand) -> Result<ExitStatus> {
    for rule in &args.ignore {
        anyhow::ensure!(
            DiagnosticCode::ALL.iter().any(|code| code.as_str() == rule),
            "unknown health rule `{rule}`"
        );
    }
    let cwd = cwd().context("health requires a Unicode working directory")?;
    // Selecting a parser grammar must not initialize Python or run sitecustomize.
    let python_version = PythonVersion::latest();
    let config_file = args
        .config
        .config_file
        .as_ref()
        .map(|path| absolute(path, &cwd));
    let mut metadata = if let Some(path) = &config_file {
        ProjectMetadata::from_config_file(path, &cwd, python_version)?
    } else {
        ProjectMetadata::discover(&cwd, python_version)?
    };
    let options = Options {
        src: Some(SrcOptions {
            include: (!args.paths.is_empty()).then_some(args.paths),
            respect_ignore_files: None,
        }),
        ..Options::default()
    };
    metadata
        .apply_overrides(
            &ProjectOptionsOverrides::new(config_file, options).with_profile(args.config.profile),
        )
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let project = Project::from_metadata(metadata);
    let settings = CollectionSettings {
        python_version,
        test_function_prefix: &project.settings().test().test_function_prefix,
        respect_ignore_files: project.settings().src().respect_ignore_files,
        collect_fixtures: true,
        collect_doctests: project.settings().test().doctest_modules,
    };
    let paths = project
        .test_paths()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let mut modules = karva_runner::ParallelCollector::new(project.cwd(), settings)
        .collect_sources(paths)?
        .into_iter()
        .map(Arc::new)
        .collect::<Vec<_>>();
    let selected_paths = modules
        .iter()
        .map(|module| module.path.path().clone())
        .collect::<Vec<_>>();
    let builtin_path = project.cwd().join(".karva/builtin-fixtures.py");
    let builtin_settings = CollectionSettings {
        python_version,
        test_function_prefix: &project.settings().test().test_function_prefix,
        respect_ignore_files: true,
        collect_fixtures: true,
        collect_doctests: false,
    };
    if let Some(module) = collect_source_with_module_name(
        &builtin_path,
        "karva._builtins",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../python/karva/_builtins.py"
        ))
        .to_owned(),
        &builtin_settings,
        &[],
    ) {
        modules.push(Arc::new(module));
    }
    let index = WorkspaceSourceIndex::from_shared_modules(
        project.cwd().clone(),
        SourceAnalysisSettings {
            python_version,
            test_function_prefix: project.settings().test().test_function_prefix.clone(),
            try_import_fixtures: project.settings().test().try_import_fixtures,
        },
        modules.iter().cloned(),
    );
    let mut findings = Vec::new();
    for path in selected_paths {
        if let Some(analysis) = index.analyze(&path) {
            for diagnostic in analysis.diagnostics {
                if !args
                    .ignore
                    .iter()
                    .any(|rule| rule == diagnostic.code.as_str())
                {
                    findings.push(source_finding(diagnostic, &modules, project.cwd()));
                }
            }
        }
    }
    findings.sort_by(|a, b| {
        (&a.location.path, a.location.start, a.rule).cmp(&(
            &b.location.path,
            b.location.start,
            b.rule,
        ))
    });
    findings.dedup_by(|a, b| {
        a.rule == b.rule
            && a.location.path == b.location.path
            && a.location.start == b.location.start
    });
    let failed = !findings.is_empty() && !matches!(args.fail_on, HealthFailOn::None);
    let report = HealthReport {
        schema_version: 1,
        analysis: "source-only",
        findings,
    };
    let mut output = io::stdout().lock();
    match args.format {
        HealthFormat::Json => {
            serde_json::to_writer_pretty(&mut output, &report)?;
            writeln!(output)?;
        }
        HealthFormat::Text => {
            for finding in &report.findings {
                writeln!(
                    output,
                    "{}:{}:{}: {}[{}]: {}",
                    finding.location.path,
                    finding.location.line,
                    finding.location.column,
                    finding.severity,
                    finding.rule,
                    finding.evidence
                )?;
                for related in &finding.related {
                    writeln!(
                        output,
                        "  {}:{}:{}: {}",
                        related.location.path,
                        related.location.line,
                        related.location.column,
                        related.evidence
                    )?;
                }
                writeln!(output, "  help: {}", finding.action)?;
            }
            writeln!(
                output,
                "{} health finding(s) (source-only; tests were not executed)",
                report.findings.len()
            )?;
        }
    }
    Ok(if failed {
        ExitStatus::Failure
    } else {
        ExitStatus::Success
    })
}

fn source_finding(
    diagnostic: SourceDiagnostic,
    modules: &[Arc<CollectedModule>],
    root: &Utf8Path,
) -> Finding {
    Finding {
        rule: diagnostic.code.as_str(),
        severity: "error",
        location: Location::new(
            &diagnostic.location.path,
            diagnostic.location.range,
            modules,
            root,
        ),
        evidence: diagnostic.message,
        related: diagnostic
            .related
            .into_iter()
            .map(|related| Related {
                location: Location::new(
                    &related.location.path,
                    related.location.range,
                    modules,
                    root,
                ),
                evidence: related.message,
            })
            .collect(),
        action: diagnostic.code.action(),
    }
}
