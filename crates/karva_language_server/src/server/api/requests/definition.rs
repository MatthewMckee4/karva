//! Fixture definition lookup for Karva source documents.

use karva_ide::fixture_definition;
use lsp_types::{DefinitionParams, DefinitionRequest, DefinitionResponse, Location, Uri};
use ruff_source_file::LineIndex;

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::{position_to_text_size, text_range_to_range};
use crate::session::client::Client;
use crate::session::{
    DocumentSnapshotVersion, PreparedSourceAnalysis, RequestCancellationToken, Session,
};

pub(in crate::server::api) struct Definition;

pub(in crate::server::api) struct DefinitionSnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    position_encoding: PositionEncoding,
}

impl RequestHandler for Definition {
    type RequestType = DefinitionRequest;
}

impl BackgroundRequestHandler for Definition {
    type Snapshot = DefinitionSnapshot;

    fn prepare(session: &mut Session, params: &DefinitionParams) -> anyhow::Result<Self::Snapshot> {
        let uri = &params.text_document_position_params.text_document.uri;
        Ok(DefinitionSnapshot {
            analysis: session.prepare_source_analysis(uri)?,
            position_encoding: session.position_encoding(),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: DefinitionParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<DefinitionResponse>> {
        let Some(prepared) = snapshot.analysis else {
            return Ok(None);
        };
        let source_text = prepared.source_text().to_owned();
        let index = LineIndex::from_source_text(&source_text);
        let offset = position_to_text_size(
            params.text_document_position_params.position,
            &source_text,
            &index,
            snapshot.position_encoding,
        );
        let Some(analysis) = prepared.analyze(cancellation)? else {
            return Ok(None);
        };
        let Some(target) = fixture_definition(&analysis.analysis, offset) else {
            return Ok(None);
        };
        let Some(target_source) = analysis
            .source_index
            .module(&target.path)
            .map(|module| module.source_text.as_str())
        else {
            tracing::debug!(path = %target.path, "definition source is not available");
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
        let Some(uri) = path_to_uri(&target.path) else {
            tracing::debug!(path = %target.path, "definition path is not a valid URI");
            return Ok(None);
        };

        Ok(Some(DefinitionResponse::Definition(
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

fn path_to_uri(path: &camino::Utf8Path) -> Option<Uri> {
    Uri::from_file_path(path.as_std_path()).ok()
}
