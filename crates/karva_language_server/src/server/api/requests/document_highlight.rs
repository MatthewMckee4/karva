//! Same-document highlighting for statically resolved Karva fixtures.

use karva_ide::{FixtureOccurrenceKind, fixture_document_highlights};
use lsp_types::{DocumentHighlightKind, DocumentHighlightParams, DocumentHighlightRequest};
use ruff_source_file::LineIndex;

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::{position_to_text_size, text_range_to_range};
use crate::session::client::Client;
use crate::session::{
    DocumentSnapshotVersion, PreparedSourceAnalysis, RequestCancellationToken, Session,
};

pub(in crate::server::api) struct DocumentHighlight;

pub(in crate::server::api) struct DocumentHighlightSnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    position_encoding: PositionEncoding,
}

impl RequestHandler for DocumentHighlight {
    type RequestType = DocumentHighlightRequest;
}

impl BackgroundRequestHandler for DocumentHighlight {
    type Snapshot = DocumentHighlightSnapshot;

    fn prepare(
        session: &mut Session,
        params: &DocumentHighlightParams,
    ) -> anyhow::Result<Self::Snapshot> {
        let uri = &params.text_document_position_params.text_document.uri;
        Ok(DocumentHighlightSnapshot {
            analysis: session.prepare_source_analysis(uri)?,
            position_encoding: session.position_encoding(),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: DocumentHighlightParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<Vec<lsp_types::DocumentHighlight>>> {
        let Some(prepared) = snapshot.analysis else {
            return Ok(None);
        };
        let source_text = prepared.source_text().to_owned();
        let line_index = LineIndex::from_source_text(&source_text);
        let offset = position_to_text_size(
            params.text_document_position_params.position,
            &source_text,
            &line_index,
            snapshot.position_encoding,
        );
        let Some(analysis) = prepared.analyze(cancellation)? else {
            return Ok(None);
        };
        let Some(highlights) = fixture_document_highlights(&analysis.analysis, offset) else {
            return Ok(None);
        };
        let highlights = highlights
            .into_iter()
            .filter_map(|occurrence| {
                let range = text_range_to_range(
                    occurrence.range,
                    &source_text,
                    &line_index,
                    snapshot.position_encoding,
                )?;
                let kind = match occurrence.kind {
                    FixtureOccurrenceKind::Definition => DocumentHighlightKind::Write,
                    FixtureOccurrenceKind::Dependency
                    | FixtureOccurrenceKind::TestParameter
                    | FixtureOccurrenceKind::UseFixtures => DocumentHighlightKind::Read,
                };
                Some(lsp_types::DocumentHighlight {
                    range,
                    kind: Some(kind),
                })
            })
            .collect();

        Ok(Some(highlights))
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        snapshot
            .analysis
            .as_ref()
            .map(PreparedSourceAnalysis::response_version)
    }
}
