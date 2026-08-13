//! Workspace references for statically resolved Karva fixtures.

use karva_ide::{fixture_references, fixture_target};
use lsp_types::{Location, ReferenceParams, ReferencesRequest, Uri};
use ruff_source_file::LineIndex;

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::{position_to_text_size, text_range_to_range};
use crate::session::client::Client;
use crate::session::{
    DocumentSnapshotVersion, PreparedSourceAnalysis, RequestCancellationToken, Session,
};

pub(in crate::server::api) struct References;

pub(in crate::server::api) struct ReferencesSnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    position_encoding: PositionEncoding,
}

impl RequestHandler for References {
    type RequestType = ReferencesRequest;
}

impl BackgroundRequestHandler for References {
    type Snapshot = ReferencesSnapshot;

    fn prepare(session: &mut Session, params: &ReferenceParams) -> anyhow::Result<Self::Snapshot> {
        let uri = &params.text_document_position_params.text_document.uri;
        Ok(ReferencesSnapshot {
            analysis: session.prepare_project_source_analysis(uri)?,
            position_encoding: session.position_encoding(),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: ReferenceParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<Vec<Location>>> {
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
        let Some(target) = fixture_target(&analysis.analysis, offset) else {
            return Ok(None);
        };

        let locations = fixture_references(
            &analysis.source_index,
            &target,
            params.context.include_declaration,
        )
        .into_iter()
        .filter_map(|reference| {
            let source = analysis
                .source_index
                .module(&reference.path)?
                .source_text
                .as_str();
            let range = text_range_to_range(
                reference.occurrence.range,
                source,
                &LineIndex::from_source_text(source),
                snapshot.position_encoding,
            )?;
            let uri = Uri::from_file_path(reference.path.as_std_path()).ok()?;
            Some(Location::new(uri, range))
        })
        .collect();

        Ok(Some(locations))
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        snapshot
            .analysis
            .as_ref()
            .map(PreparedSourceAnalysis::response_version)
    }
}
