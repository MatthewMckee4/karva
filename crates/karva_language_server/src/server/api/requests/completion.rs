//! Fixture-name completion from source-only Karva analysis.

use karva_ide::complete_fixtures;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionParams, CompletionRequest,
    CompletionResponse, TextEdit,
};
use ruff_source_file::LineIndex;

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::{position_to_text_size, text_range_to_range};
use crate::session::client::Client;
use crate::session::{PreparedSourceAnalysis, RequestCancellationToken, Session};

pub(in crate::server::api) struct Completion;

pub(in crate::server::api) struct CompletionSnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    position_encoding: PositionEncoding,
}

impl RequestHandler for Completion {
    type RequestType = CompletionRequest;
}

impl BackgroundRequestHandler for Completion {
    type Snapshot = CompletionSnapshot;

    fn prepare(session: &mut Session, params: &CompletionParams) -> anyhow::Result<Self::Snapshot> {
        let uri = &params.text_document_position_params.text_document.uri;
        Ok(CompletionSnapshot {
            analysis: session.prepare_source_analysis(uri)?,
            position_encoding: session.position_encoding(),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: CompletionParams,
        _cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<CompletionResponse>> {
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
        let Some(analysis) = prepared.analyze()? else {
            return Ok(None);
        };
        let Some(completions) = complete_fixtures(&analysis.analysis, offset) else {
            return Ok(None);
        };

        let items = completions
            .into_iter()
            .filter_map(|completion| {
                Some(CompletionItem {
                    label: completion.label.clone(),
                    kind: Some(CompletionItemKind::Variable),
                    detail: Some(completion.detail),
                    text_edit: Some(
                        TextEdit::new(
                            text_range_to_range(
                                completion.replacement_range,
                                &source_text,
                                &index,
                                snapshot.position_encoding,
                            )?,
                            completion.label,
                        )
                        .into(),
                    ),
                    ..CompletionItem::default()
                })
            })
            .collect();

        Ok(Some(CompletionResponse::CompletionList(CompletionList {
            is_incomplete: false,
            items,
            item_defaults: None,
            apply_kind: None,
        })))
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<(lsp_types::Uri, i32)> {
        snapshot
            .analysis
            .as_ref()
            .map(|analysis| (analysis.document_uri().clone(), analysis.document_version()))
    }
}
