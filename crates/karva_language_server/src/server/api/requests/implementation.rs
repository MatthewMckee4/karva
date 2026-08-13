//! Fixture implementation lookup for Karva source documents.

use karva_ide::fixture_implementation;
use lsp_types::{
    ImplementationParams, ImplementationRequest, ImplementationResponse, Location, Uri,
};
use ruff_source_file::LineIndex;

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::{position_to_text_size, text_range_to_range};
use crate::session::client::Client;
use crate::session::{
    DocumentSnapshotVersion, PreparedSourceAnalysis, RequestCancellationToken, Session,
};

pub(in crate::server::api) struct Implementation;

pub(in crate::server::api) struct ImplementationSnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    position_encoding: PositionEncoding,
}

impl RequestHandler for Implementation {
    type RequestType = ImplementationRequest;
}

impl BackgroundRequestHandler for Implementation {
    type Snapshot = ImplementationSnapshot;

    fn prepare(
        session: &mut Session,
        params: &ImplementationParams,
    ) -> anyhow::Result<Self::Snapshot> {
        let uri = &params.text_document_position_params.text_document.uri;
        Ok(ImplementationSnapshot {
            analysis: session.prepare_source_analysis(uri)?,
            position_encoding: session.position_encoding(),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: ImplementationParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<ImplementationResponse>> {
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
        let Some(target) = fixture_implementation(&analysis.analysis, offset) else {
            return Ok(None);
        };
        let Some(target_source) = analysis
            .source_index
            .module(&target.path)
            .map(|module| module.source_text.as_str())
        else {
            tracing::debug!(path = %target.path, "implementation source is not available");
            return Ok(None);
        };
        let Some(target_range) = text_range_to_range(
            target.range,
            target_source,
            &LineIndex::from_source_text(target_source),
            snapshot.position_encoding,
        ) else {
            return Ok(None);
        };
        let Ok(uri) = Uri::from_file_path(target.path.as_std_path()) else {
            tracing::debug!(path = %target.path, "implementation path is not a valid URI");
            return Ok(None);
        };

        Ok(Some(ImplementationResponse::Definition(
            lsp_types::Definition::Location(Location::new(uri, target_range)),
        )))
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        snapshot
            .analysis
            .as_ref()
            .map(PreparedSourceAnalysis::response_version)
    }
}
