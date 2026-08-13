//! Push diagnostics derived from source-only Karva analysis.

use std::collections::{BTreeMap, HashMap, HashSet};

use camino::{Utf8Path, Utf8PathBuf};
use karva_ide::{SourceDiagnostic, SourceLocation};
use lsp_types::{
    Code, Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location,
    PublishDiagnosticsNotification, PublishDiagnosticsParams, Range, Uri,
};
use ruff_source_file::LineIndex;

use crate::document::text_range_to_range;
use crate::session::client::Client;
use crate::session::{DiagnosticAnalysis, PreparedDiagnostics, Session, SourceIndexRevision};

/// Fully converted diagnostics awaiting stale-result validation on the event loop.
#[derive(Debug)]
pub struct DiagnosticPublication {
    current_paths: HashSet<Utf8PathBuf>,
    diagnostics_by_path: BTreeMap<Utf8PathBuf, Vec<Diagnostic>>,
    pub(crate) source_index_revision: SourceIndexRevision,
}

/// Reads, analyzes, sorts, and converts diagnostics on the latest-only worker.
pub(super) fn compute_diagnostics(
    prepared: PreparedDiagnostics,
    position_encoding: crate::PositionEncoding,
    supports_related_information: bool,
) -> Option<DiagnosticPublication> {
    let DiagnosticAnalysis {
        open_python_paths,
        mut diagnostics_by_path,
        mut errors_by_path,
        sources,
        source_index_revision,
        cancellation,
    } = prepared.analyze()?;

    for diagnostics in diagnostics_by_path.values_mut() {
        if cancellation.is_cancelled() {
            return None;
        }
        diagnostics.sort_by(|left, right| {
            left.location
                .range
                .start()
                .cmp(&right.location.range.start())
                .then_with(|| left.location.range.end().cmp(&right.location.range.end()))
                .then_with(|| left.code.as_str().cmp(right.code.as_str()))
                .then_with(|| left.message.cmp(&right.message))
        });
        diagnostics.dedup();
    }

    let mut line_indexes = HashMap::new();
    for (path, source) in &sources {
        if cancellation.is_cancelled() {
            return None;
        }
        line_indexes.insert(path.clone(), LineIndex::from_source_text(source));
    }
    let mut converted = BTreeMap::new();
    for (path, diagnostics) in diagnostics_by_path {
        if cancellation.is_cancelled() {
            return None;
        }
        let mut lsp_diagnostics = Vec::with_capacity(diagnostics.len());
        for diagnostic in diagnostics {
            if cancellation.is_cancelled() {
                return None;
            }
            if let Some(diagnostic) = source_diagnostic_to_lsp(
                diagnostic,
                &sources,
                &line_indexes,
                position_encoding,
                supports_related_information,
            ) {
                lsp_diagnostics.push(diagnostic);
            }
        }
        converted.insert(path, lsp_diagnostics);
    }
    for (path, errors) in &mut errors_by_path {
        converted
            .entry(path.clone())
            .or_insert_with(Vec::new)
            .extend(errors.drain(..).map(configuration_diagnostic));
    }

    let mut current_paths = open_python_paths;
    current_paths.extend(converted.keys().cloned());
    Some(DiagnosticPublication {
        current_paths,
        diagnostics_by_path: converted,
        source_index_revision,
    })
}

/// Publishes completed diagnostics after the main loop validates their revision.
pub(in crate::server) fn publish_diagnostics(
    session: &mut Session,
    client: &Client,
    publication: DiagnosticPublication,
) -> anyhow::Result<()> {
    let DiagnosticPublication {
        current_paths,
        mut diagnostics_by_path,
        source_index_revision: _,
    } = publication;
    let previous_paths = session.replace_published_diagnostic_paths(current_paths.clone());
    let mut paths_to_publish = current_paths;
    paths_to_publish.extend(previous_paths);
    let mut paths_to_publish = paths_to_publish.into_iter().collect::<Vec<_>>();
    paths_to_publish.sort();

    for path in paths_to_publish {
        let Some(uri) = path_to_uri(session, &path) else {
            tracing::warn!(%path, "cannot convert diagnostic path to URI");
            continue;
        };
        let version = session.document(&uri).map(crate::TextDocument::version);
        let diagnostics = diagnostics_by_path.remove(&path).unwrap_or_default();

        client.send_notification::<PublishDiagnosticsNotification>(
            PublishDiagnosticsParams::new(uri, version, diagnostics),
        )?;
    }

    Ok(())
}

fn source_diagnostic_to_lsp(
    diagnostic: SourceDiagnostic,
    sources: &HashMap<Utf8PathBuf, String>,
    line_indexes: &HashMap<Utf8PathBuf, LineIndex>,
    position_encoding: crate::PositionEncoding,
    supports_related_information: bool,
) -> Option<Diagnostic> {
    let range = source_location_to_range(
        &diagnostic.location,
        sources,
        line_indexes,
        position_encoding,
    )?;
    let related_information = supports_related_information.then(|| {
        diagnostic
            .related
            .into_iter()
            .filter_map(|related| {
                let uri = Uri::from_file_path(related.location.path.as_std_path()).ok()?;
                let range = source_location_to_range(
                    &related.location,
                    sources,
                    line_indexes,
                    position_encoding,
                )?;
                Some(DiagnosticRelatedInformation::new(
                    Location::new(uri, range),
                    related.message,
                ))
            })
            .collect()
    });

    Some(Diagnostic::new(
        range,
        Some(DiagnosticSeverity::Error),
        Some(Code::String(diagnostic.code.as_str().to_owned())),
        None,
        Some("karva".to_owned()),
        diagnostic.message.into(),
        None,
        related_information,
        None,
    ))
}

fn source_location_to_range(
    location: &SourceLocation,
    sources: &HashMap<Utf8PathBuf, String>,
    line_indexes: &HashMap<Utf8PathBuf, LineIndex>,
    encoding: crate::PositionEncoding,
) -> Option<Range> {
    let source_text = sources.get(&location.path)?;
    let line_index = line_indexes.get(&location.path)?;
    text_range_to_range(location.range, source_text, line_index, encoding)
}

fn configuration_diagnostic(message: String) -> Diagnostic {
    Diagnostic::new(
        Range::default(),
        Some(DiagnosticSeverity::Error),
        Some(Code::String("configuration-error".to_owned())),
        None,
        Some("karva".to_owned()),
        message.into(),
        None,
        None,
        None,
    )
}

fn path_to_uri(session: &Session, path: &Utf8Path) -> Option<Uri> {
    session
        .document_uri_for_path(path)
        .or_else(|| Uri::from_file_path(path.as_std_path()).ok())
}
