//! Workspace edits for statically resolved Karva fixture renames.

use std::collections::HashMap;

use anyhow::{Context, anyhow};
use karva_ide::{fixture_rename_target, is_valid_fixture_name, rename_fixture};
use lsp_types::{RenameParams, RenameRequest, TextEdit, Uri, WorkspaceEdit};
use ruff_source_file::LineIndex;

use super::super::RequestError;
use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::{position_to_text_size, text_range_to_range};
use crate::session::client::Client;
use crate::session::{
    DocumentSnapshotVersion, PreparedSourceAnalysis, RequestCancellationToken, Session,
};

pub(in crate::server::api) struct Rename;

pub(in crate::server::api) struct RenameSnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    position_encoding: PositionEncoding,
}

impl RequestHandler for Rename {
    type RequestType = RenameRequest;
}

impl BackgroundRequestHandler for Rename {
    type Snapshot = RenameSnapshot;

    fn prepare(session: &mut Session, params: &RenameParams) -> anyhow::Result<Self::Snapshot> {
        let uri = &params.text_document_position_params.text_document.uri;
        Ok(RenameSnapshot {
            analysis: session.prepare_project_source_analysis(uri)?,
            position_encoding: session.position_encoding(),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: RenameParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<WorkspaceEdit>> {
        if !is_valid_fixture_name(&params.new_name) {
            return Err(RequestError::invalid_params(format!(
                "fixture rename `{}` is not a valid Python identifier",
                params.new_name
            ))
            .into());
        }
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
        let Some(occurrences) =
            rename_fixture(&analysis.source_index, &target.occurrence, &params.new_name)
        else {
            return Ok(None);
        };

        let mut changes = HashMap::new();
        for occurrence in occurrences {
            let edit_range = occurrence
                .occurrence
                .edit_range
                .context("fixture rename preflight returned an uneditable occurrence")?;
            let source = analysis
                .source_index
                .module(&occurrence.path)
                .context("fixture rename source is missing from the project snapshot")?
                .source_text
                .as_str();
            let range = text_range_to_range(
                edit_range,
                source,
                &LineIndex::from_source_text(source),
                snapshot.position_encoding,
            )
            .context("fixture rename range is outside its source")?;
            let uri = Uri::from_file_path(occurrence.path.as_std_path())
                .map_err(|()| anyhow!("fixture rename path cannot be converted to a file URI"))?;
            changes
                .entry(uri)
                .or_insert_with(Vec::new)
                .push(TextEdit::new(range, params.new_name.clone()));
        }

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }))
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        snapshot
            .analysis
            .as_ref()
            .map(PreparedSourceAnalysis::response_version)
    }
}
