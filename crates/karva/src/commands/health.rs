//! Source inspection shares project resolution and collection with the runner.
//! Python is never imported here; dynamic behavior remains unknown.

use std::io::{self, Write};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use camino::Utf8Path;
use karva_analysis::{
    DiagnosticCode, SourceAnalysisSettings, SourceDiagnostic, WorkspaceSourceIndex,
};
use karva_cli::{HealthCommand, HealthFailOn, HealthFormat};
use karva_collector::{CollectedModule, CollectionSettings, collect_source_with_module_name};
use karva_metadata::{
    Options, ProjectMetadata, ProjectOptionsOverrides, SrcOptions, TerminalOptions,
};
use karva_project::Project;
use karva_project::path::absolute;
use ruff_python_ast::PythonVersion;
use ruff_text_size::TextRange;
use serde::Serialize;

mod probe;
mod runtime;

use crate::ExitStatus;
use crate::utils::cwd;

/// Versioned, deterministic machine interface for suite inspection.
#[derive(Serialize)]
struct HealthReport {
    schema_version: u8,
    analysis: &'static str,
    findings: Vec<Finding>,

    #[serde(skip_serializing_if = "Option::is_none")]
    runtime: Option<runtime::RuntimeReport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    probe: Option<probe::ProbeReport>,
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

    #[serde(skip_serializing_if = "Option::is_none")]
    reproduction: Option<Vec<String>>,
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
    if let Some(count) = args.probe {
        anyhow::ensure!(
            count.get() >= 2,
            "health probe requires at least 2 runs per order, requested {}",
            count.get()
        );
    }
    let mut reproduction = vec![
        "uv".to_owned(),
        "run".to_owned(),
        "karva".to_owned(),
        "health".to_owned(),
    ];
    reproduction.extend(args.paths.iter().cloned());
    if let Some(profile) = &args.config.profile {
        reproduction.extend(["--profile".to_owned(), profile.clone()]);
    }
    for rule in &args.ignore {
        anyhow::ensure!(
            DiagnosticCode::ALL.iter().any(|code| code.as_str() == rule)
                || matches!(
                    rule.as_str(),
                    "duration-hotspot" | "unstable-test" | "order-dependent-test"
                ),
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
    if let Some(path) = &config_file {
        reproduction.extend(["--config-file".to_owned(), path.to_string()]);
    }
    let mut metadata = if let Some(path) = &config_file {
        ProjectMetadata::from_config_file(path, &cwd, python_version)?
    } else {
        ProjectMetadata::discover(&cwd, python_version)?
    };
    let full_suite = args.paths.is_empty();
    let options = Options {
        test: args.probe.map(|_| karva_metadata::TestOptions {
            retry: Some(0),
            fail_fast: Some(false),
            max_fail: Some(karva_metadata::MaxFail::unlimited()),
            ..karva_metadata::TestOptions::default()
        }),
        terminal: Some(TerminalOptions {
            status_level: Some(karva_logging::StatusLevel::None),
            final_status_level: Some(karva_logging::FinalStatusLevel::None),
            show_python_output: Some(false),
            output_format: None,
        }),
        src: Some(SrcOptions {
            include: (!args.paths.is_empty()).then_some(args.paths),
            respect_ignore_files: None,
        }),
        ..Options::default()
    };
    metadata
        .apply_overrides(
            &ProjectOptionsOverrides::new(config_file, options)
                .with_profile(args.config.profile.clone()),
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
        let Some(analysis) = index.analyze(&path) else {
            continue;
        };
        if let Some(module) = index.module(&path) {
            for diagnostic in karva_analysis::shadowed_definitions(
                module,
                &project.settings().test().test_function_prefix,
                &analysis.diagnostics,
            )
            .into_iter()
            .chain(karva_analysis::duplicate_parameter_cases(module))
            {
                if !args
                    .ignore
                    .iter()
                    .any(|rule| rule == diagnostic.code.as_str())
                {
                    findings.push(source_finding(diagnostic, &modules, project.cwd()));
                }
            }
        }
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
    if full_suite {
        for diagnostic in karva_analysis::unused_fixtures(&index) {
            if !args
                .ignore
                .iter()
                .any(|rule| rule == diagnostic.code.as_str())
            {
                findings.push(source_finding(diagnostic, &modules, project.cwd()));
            }
        }
    }
    let runtime = if args.durations {
        let report = runtime::durations(&project, args.config.profile.clone())?;
        if !args.ignore.iter().any(|rule| rule == "duration-hotspot") {
            findings.extend(report.hotspots());
        }
        Some(report)
    } else {
        None
    };
    let probe = if let Some(count) = args.probe {
        let report = probe::probe(
            &project,
            args.config.profile.as_deref(),
            count.get(),
            args.probe_seed,
            reproduction,
        )?;
        findings.extend(
            report
                .findings(&modules, project.cwd())
                .into_iter()
                .filter(|finding| !args.ignore.iter().any(|rule| rule == finding.rule)),
        );
        Some(report)
    } else {
        None
    };
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
    let failed = probe.as_ref().is_some_and(|report| !report.success)
        || runtime.as_ref().is_some_and(|report| !report.success)
        || findings.iter().any(|finding| match args.fail_on {
            HealthFailOn::None => false,
            HealthFailOn::Warning => finding.severity != "info",
            HealthFailOn::Error => finding.severity == "error",
        });
    let report = HealthReport {
        schema_version: 1,
        analysis: if runtime.is_some() || probe.is_some() {
            "source-and-runtime"
        } else {
            "source-only"
        },
        findings,
        runtime,
        probe,
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
                if let Some(argv) = &finding.reproduction {
                    writeln!(output, "  reproduce argv: {}", serde_json::to_string(argv)?)?;
                }
            }
            if let Some(probe) = &report.probe {
                writeln!(
                    output,
                    "{} health finding(s) (probe: {} observed run(s); seeds: {:?})",
                    report.findings.len(),
                    probe.runs.len(),
                    probe.seeds
                )?;
                let errors = probe
                    .runs
                    .iter()
                    .flat_map(|run| &run.errors)
                    .collect::<std::collections::BTreeSet<_>>();
                for error in errors {
                    writeln!(output, "{error}")?;
                }
            } else if let Some(runtime) = &report.runtime {
                writeln!(
                    output,
                    "{} health finding(s) (runtime measured; {} invocation(s); median {:.6}s; reporting overhead {:.6}s)",
                    report.findings.len(),
                    runtime.measurements.len(),
                    runtime.median_seconds.unwrap_or_default(),
                    runtime.reporting_overhead_seconds
                )?;
                for diagnostic in &runtime.errors {
                    writeln!(output, "{diagnostic}")?;
                }
            } else {
                writeln!(
                    output,
                    "{} health finding(s) (source-only; tests were not executed)",
                    report.findings.len()
                )?;
            }
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
        severity: if matches!(
            diagnostic.code,
            DiagnosticCode::UnusedFixture
                | DiagnosticCode::PossiblyShadowedDefinition
                | DiagnosticCode::DuplicateParameterCase
                | DiagnosticCode::DuplicateParameterId
        ) {
            "warning"
        } else {
            "error"
        },
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
        reproduction: None,
    }
}
