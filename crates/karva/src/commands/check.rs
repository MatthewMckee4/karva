//! Source-only health command: resolve project inputs, analyze, then render.

use std::collections::BTreeSet;
use std::io::{self, Write as _};

use anyhow::{Context as _, Result};
use camino::{Utf8Path, Utf8PathBuf};
use karva_cli::{CheckCommand, CheckFormat};
use karva_collector::{CollectionSettings, collect_source};
use karva_diagnostic::{
    Annotation, Diagnostic, DiagnosticFormat, DisplayDiagnosticConfig, Severity, Span,
    render_diagnostic,
};
use karva_ide::{SourceDiagnostic, source_health};
use karva_metadata::{Options, ProjectMetadata, ProjectOptionsOverrides};
use karva_project::{
    Project,
    path::{TestPath, absolute},
    python_source_walker,
};
use ruff_python_ast::PythonVersion;
use ruff_source_file::{SourceFile, SourceFileBuilder};
use serde::Serialize;

use crate::{ExitStatus, utils::cwd};

/// Machine-readable span; end positions are exclusive and columns count Unicode scalars.
#[derive(Serialize)]
struct JsonLocation {
    path: String,
    byte_range: [u32; 2],
    start: JsonPosition,
    end: JsonPosition,
}

#[derive(Serialize)]
struct JsonPosition {
    line: usize,
    column: usize,
}

#[derive(Serialize)]
struct JsonRelated<'a> {
    message: &'a str,
    #[serde(flatten)]
    location: JsonLocation,
}

#[derive(Serialize)]
struct JsonFinding<'a> {
    code: &'a str,
    message: &'a str,
    #[serde(flatten)]
    location: JsonLocation,
    related: Vec<JsonRelated<'a>>,
}

/// Checks selected test sources without starting Python or workers.
pub fn check(args: CheckCommand) -> Result<ExitStatus> {
    let cwd = cwd().context("current directory is not UTF-8")?;
    let config_file = args
        .config
        .config_file
        .as_ref()
        .map(|path| absolute(path, &cwd));
    // Source checks intentionally do not initialize Python to discover its version.
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
    let paths = discover_paths(&project, &args.paths, &cwd)?;
    let settings = CollectionSettings {
        python_version: project.metadata().python_version(),
        test_function_prefix: &project.settings().test().test_function_prefix,
        respect_ignore_files: project.settings().src().respect_ignore_files,
        collect_fixtures: true,
        collect_doctests: false,
    };
    let mut analyzed = Vec::new();
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
        let findings = source_health(&module, settings.test_function_prefix);
        if !findings.is_empty() {
            let source_file =
                SourceFileBuilder::new(path.as_str(), module.source_text.as_str()).finish();
            analyzed.push((path, source_file, findings));
        }
    }
    render(&analyzed, args.output_format, &cwd)?;
    Ok(if analyzed.is_empty() {
        ExitStatus::Success
    } else {
        ExitStatus::Failure
    })
}

/// Shares default selections and directory filtering with runtime collection.
fn discover_paths(
    project: &Project,
    explicit: &[Utf8PathBuf],
    cwd: &Utf8Path,
) -> Result<BTreeSet<Utf8PathBuf>> {
    let roots = if explicit.is_empty() {
        project
            .test_paths()
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
    } else {
        explicit
            .iter()
            .map(|path| TestPath::new(absolute(path, cwd).as_str()))
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut paths = BTreeSet::new();
    for root in roots {
        match root {
            TestPath::Directory(root) => {
                for entry in
                    python_source_walker(&root, project.settings().src().respect_ignore_files)?
                        .build()
                {
                    let entry =
                        entry.with_context(|| format!("failed to walk source root {root}"))?;
                    if entry.file_type().is_some_and(|kind| kind.is_file())
                        && let Ok(path) = Utf8PathBuf::from_path_buf(entry.into_path())
                    {
                        paths.insert(path);
                    }
                }
            }
            TestPath::File(path) => {
                paths.insert(path);
            }
            TestPath::Function(_) => {
                anyhow::bail!("source checks accept files or directories, not function selectors")
            }
        }
    }
    Ok(paths)
}

fn render(
    analyzed: &[(Utf8PathBuf, SourceFile, Vec<SourceDiagnostic>)],
    format: CheckFormat,
    cwd: &Utf8Path,
) -> Result<()> {
    let mut stdout = io::stdout().lock();
    match format {
        CheckFormat::Json => {
            let mut output = Vec::new();
            for (path, source, findings) in analyzed {
                let location = |range: karva_ide::SourceLocation| {
                    let start = source.to_source_code().line_column(range.range.start());
                    let end = source.to_source_code().line_column(range.range.end());
                    JsonLocation {
                        path: json_path(path.strip_prefix(cwd).unwrap_or(path)),
                        byte_range: [u32::from(range.range.start()), u32::from(range.range.end())],
                        start: JsonPosition {
                            line: start.line.get(),
                            column: start.column.get(),
                        },
                        end: JsonPosition {
                            line: end.line.get(),
                            column: end.column.get(),
                        },
                    }
                };
                for finding in findings {
                    output.push(JsonFinding {
                        code: finding.code.as_str(),
                        message: &finding.message,
                        location: location(finding.location.clone()),
                        related: finding
                            .related
                            .iter()
                            .map(|related| JsonRelated {
                                message: &related.message,
                                location: location(related.location.clone()),
                            })
                            .collect(),
                    });
                }
            }
            serde_json::to_writer_pretty(&mut stdout, &output)?;
            writeln!(stdout)?;
        }
        CheckFormat::Text => {
            for (_, source, findings) in analyzed {
                for finding in findings {
                    let mut diagnostic = Diagnostic::new(
                        finding.code.as_str(),
                        Severity::Warning,
                        finding.message.clone(),
                    );
                    diagnostic.annotate(Annotation::primary(
                        Span::from(source.clone()).with_range(finding.location.range),
                    ));
                    for related in &finding.related {
                        diagnostic.annotate(
                            Annotation::secondary(
                                Span::from(source.clone()).with_range(related.location.range),
                            )
                            .message(related.message.clone()),
                        );
                    }
                    let rendered = render_diagnostic(
                        &diagnostic,
                        cwd,
                        DisplayDiagnosticConfig::new(
                            DiagnosticFormat::Full,
                            colored::control::SHOULD_COLORIZE.should_colorize(),
                        ),
                    );
                    write!(stdout, "{}", rendered.rendered_for_terminal())?;
                }
            }
        }
    }
    Ok(())
}

/// Normalizes native separators without changing valid Unix filename characters.
fn json_path(path: &Utf8Path) -> String {
    if cfg!(windows) {
        path.as_str().replace('\\', "/")
    } else {
        path.to_string()
    }
}
