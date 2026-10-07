//! Range-limited framework annotations for fixture injection sites.

use karva_ide::fixture_reference_hovers;
use lsp_types::{InlayHint, InlayHintParams, InlayHintRequest};
use ruff_source_file::LineIndex;

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::text_range_to_range;
use crate::session::client::Client;
use crate::session::{
    DocumentSnapshotVersion, PreparedSourceAnalysis, RequestCancellationToken, Session,
};

pub(in crate::server::api) struct InlayHints;

pub(in crate::server::api) struct InlayHintsSnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    position_encoding: PositionEncoding,
}

impl RequestHandler for InlayHints {
    type RequestType = InlayHintRequest;
}

impl BackgroundRequestHandler for InlayHints {
    type Snapshot = InlayHintsSnapshot;

    fn prepare(session: &mut Session, params: &InlayHintParams) -> anyhow::Result<Self::Snapshot> {
        Ok(InlayHintsSnapshot {
            analysis: session.prepare_source_analysis(&params.text_document.uri)?,
            position_encoding: session.position_encoding(),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: InlayHintParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<Vec<InlayHint>>> {
        let Some(prepared) = snapshot.analysis else {
            return Ok(None);
        };
        let source = prepared.source_text().to_owned();
        let line_index = LineIndex::from_source_text(&source);
        let Some(analysis) = prepared.analyze(cancellation)? else {
            return Ok(None);
        };
        let mut hints = Vec::new();
        for hover in fixture_reference_hovers(&analysis.analysis) {
            if cancellation.is_cancelled() {
                return Ok(None);
            }
            let Some(range) = text_range_to_range(
                hover.range,
                &source,
                &line_index,
                snapshot.position_encoding,
            ) else {
                continue;
            };
            let position = range.end;
            if position < params.range.start || position >= params.range.end {
                continue;
            }
            let scope = hover
                .scope
                .map_or("dynamic", karva_ide::FixtureScope::as_str);
            let provider = hover.provider.as_ref().map_or("built-in", |id| {
                id.path.file_name().unwrap_or(id.path.as_str())
            });
            let provider_path = hover
                .provider
                .as_ref()
                .map_or("Karva built-in", |id| id.path.as_str());
            hints.push(InlayHint {
                position,
                label: format!("fixture: {scope} ({provider})").into(),
                kind: None,
                text_edits: None,
                tooltip: Some(
                    format!(
                        "Karva fixture `{}`\nScope: {scope}\nProvider: {provider_path}",
                        hover.name
                    )
                    .into(),
                ),
                padding_left: Some(true),
                padding_right: None,
                data: None,
            });
        }
        Ok(Some(hints))
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        snapshot
            .analysis
            .as_ref()
            .map(PreparedSourceAnalysis::response_version)
    }
}
