//! Rename preflight for statically resolved Karva fixtures.

use karva_ide::{fixture_rename_target, prepare_fixture_rename};
use lsp_types::{
    PrepareRenameParams, PrepareRenamePlaceholder, PrepareRenameRequest, PrepareRenameResult,
};
use ruff_source_file::LineIndex;

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::{position_to_text_size, text_range_to_range};
use crate::session::client::Client;
use crate::session::{
    DocumentSnapshotVersion, PreparedSourceAnalysis, RequestCancellationToken, Session,
};

pub(in crate::server::api) struct PrepareRename;

pub(in crate::server::api) struct PrepareRenameSnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    position_encoding: PositionEncoding,
}

impl RequestHandler for PrepareRename {
    type RequestType = PrepareRenameRequest;
}

impl BackgroundRequestHandler for PrepareRename {
    type Snapshot = PrepareRenameSnapshot;

    fn prepare(
        session: &mut Session,
        params: &PrepareRenameParams,
    ) -> anyhow::Result<Self::Snapshot> {
        let uri = &params.text_document_position_params.text_document.uri;
        Ok(PrepareRenameSnapshot {
            analysis: session.prepare_project_source_analysis(uri)?,
            position_encoding: session.position_encoding(),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: PrepareRenameParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<PrepareRenameResult>> {
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
        let Some(target) = fixture_rename_target(&analysis.analysis, offset) else {
            return Ok(None);
        };
        let Some(range) = prepare_fixture_rename(&analysis.source_index, &target.occurrence) else {
            return Ok(None);
        };
        let Some(range) =
            text_range_to_range(range, &source_text, &line_index, snapshot.position_encoding)
        else {
            return Ok(None);
        };

        Ok(Some(PrepareRenameResult::PrepareRenamePlaceholder(
            PrepareRenamePlaceholder::new(range, target.placeholder),
        )))
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        snapshot
            .analysis
            .as_ref()
            .map(PreparedSourceAnalysis::response_version)
    }
}
