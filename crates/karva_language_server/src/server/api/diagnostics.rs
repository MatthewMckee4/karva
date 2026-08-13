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
use crate::session::Session;
use crate::session::client::Client;
use crate::workspace::uri_to_path;

/// Reanalyzes every open Python document and replaces all diagnostics previously
/// published by Karva.
pub(super) fn publish_diagnostics(session: &mut Session, client: &Client) -> anyhow::Result<()> {
    let document_uris = session.open_document_uris().cloned().collect::<Vec<_>>();
    let mut diagnostics_by_path = BTreeMap::<Utf8PathBuf, Vec<SourceDiagnostic>>::new();
    let mut errors_by_path = BTreeMap::<Utf8PathBuf, Vec<String>>::new();
    let mut open_python_paths = HashSet::new();
    let mut sources = HashMap::<Utf8PathBuf, String>::new();

    for uri in document_uris {
        let Some(document) = session.document(&uri) else {
            continue;
        };
        if document.language_id() != &lsp_types::LanguageKind::Python {
            continue;
        }

        let path = match uri_to_path(&uri) {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(%error, "cannot publish diagnostics for document");
                continue;
            }
        };
        open_python_paths.insert(path.clone());

        match session.analyze_open_document_for_diagnostics(&uri) {
            Ok(Some(snapshot)) => {
                sources.extend(snapshot.sources);
                for diagnostic in snapshot.analysis.diagnostics {
                    diagnostics_by_path
                        .entry(diagnostic.location.path.clone())
                        .or_default()
                        .push(diagnostic);
                }
            }
            Ok(None) => {}
            Err(error) => {
                errors_by_path
                    .entry(path)
                    .or_default()
                    .push(error.to_string());
            }
        }
    }

    for diagnostics in diagnostics_by_path.values_mut() {
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

    let mut current_paths = open_python_paths;
    current_paths.extend(diagnostics_by_path.keys().cloned());
    current_paths.extend(errors_by_path.keys().cloned());
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
        let diagnostics = diagnostics_by_path
            .remove(&path)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|diagnostic| {
                source_diagnostic_to_lsp(
                    session,
                    diagnostic,
                    &sources,
                    session.supports_diagnostic_related_information(),
                )
            })
            .chain(
                errors_by_path
                    .remove(&path)
                    .unwrap_or_default()
                    .into_iter()
                    .map(configuration_diagnostic),
            )
            .collect();

        client.send_notification::<PublishDiagnosticsNotification>(
            PublishDiagnosticsParams::new(uri, version, diagnostics),
        )?;
    }

    Ok(())
}

fn source_diagnostic_to_lsp(
    session: &Session,
    diagnostic: SourceDiagnostic,
    sources: &HashMap<Utf8PathBuf, String>,
    supports_related_information: bool,
) -> Option<Diagnostic> {
    let source_text = sources.get(&diagnostic.location.path)?;
    let range = source_location_to_range(
        &diagnostic.location,
        source_text,
        session.position_encoding(),
    )?;
    let related_information = supports_related_information.then(|| {
        diagnostic
            .related
            .into_iter()
            .filter_map(|related| {
                let source_text = sources.get(&related.location.path)?;
                let uri = path_to_uri(session, &related.location.path)?;
                let range = source_location_to_range(
                    &related.location,
                    source_text,
                    session.position_encoding(),
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
    source_text: &str,
    encoding: crate::PositionEncoding,
) -> Option<Range> {
    let index = LineIndex::from_source_text(source_text);
    text_range_to_range(location.range, source_text, &index, encoding)
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
