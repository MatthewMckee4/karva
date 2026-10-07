//! Versioned local edits derived from current Karva diagnostics.

use karva_ide::fixture_code_actions;
use lsp_types::{
    Code, CodeAction, CodeActionKind, CodeActionParams, CodeActionRequest, CodeActionResponse,
    Diagnostic, DiagnosticSeverity, OptionalVersionedTextDocumentIdentifier, TextDocumentEdit,
    TextEdit, WorkspaceEdit,
};
use ruff_source_file::LineIndex;
use ruff_text_size::TextRange;

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::text_range_to_range;
use crate::session::client::Client;
use crate::session::{
    DocumentSnapshotVersion, PreparedSourceAnalysis, RequestCancellationToken, Session,
};

pub(in crate::server::api) struct CodeActions;

pub(in crate::server::api) struct CodeActionsSnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    encoding: PositionEncoding,
    version: Option<i32>,
}

impl RequestHandler for CodeActions {
    type RequestType = CodeActionRequest;
}

impl BackgroundRequestHandler for CodeActions {
    type Snapshot = CodeActionsSnapshot;

    fn prepare(session: &mut Session, params: &CodeActionParams) -> anyhow::Result<Self::Snapshot> {
        Ok(CodeActionsSnapshot {
            analysis: if session.supports_versioned_code_actions() {
                session.prepare_source_analysis(&params.text_document.uri)?
            } else {
                None
            },
            encoding: session.position_encoding(),
            version: session
                .document(&params.text_document.uri)
                .map(crate::TextDocument::version),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: CodeActionParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<Vec<CodeActionResponse>>> {
        if params.context.only.as_ref().is_some_and(|kinds| {
            !kinds
                .iter()
                .any(|kind| kind.as_str().is_empty() || kind == &CodeActionKind::QuickFix)
        }) {
            return Ok(Some(Vec::new()));
        }
        let Some(prepared) = snapshot.analysis else {
            return Ok(Some(Vec::new()));
        };
        let source = prepared.source_text().to_owned();
        let index = LineIndex::from_source_text(&source);
        let Some(analysis) = prepared.analyze(cancellation)? else {
            return Ok(None);
        };
        let mut actions = Vec::new();
        for action in fixture_code_actions(&analysis.analysis) {
            if cancellation.is_cancelled() {
                return Ok(None);
            }
            let Some(range) = text_range_to_range(
                action.diagnostic.location.range,
                &source,
                &index,
                snapshot.encoding,
            ) else {
                continue;
            };
            if range.end < params.range.start || range.start > params.range.end {
                continue;
            }
            let diagnostic = Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::Error),
                code: Some(Code::String(action.diagnostic.code.as_str().to_owned())),
                source: Some("karva".to_owned()),
                message: action.diagnostic.message.into(),
                ..Diagnostic::default()
            };
            if !params.context.diagnostics.is_empty()
                && !params.context.diagnostics.iter().any(|supplied| {
                    supplied.range == diagnostic.range
                        && supplied.code == diagnostic.code
                        && supplied.message == diagnostic.message
                        && supplied.source == diagnostic.source
                })
            {
                continue;
            }
            let Some(edit_range) =
                text_range_to_range(action.range, &source, &index, snapshot.encoding)
            else {
                continue;
            };
            let Some(import_range) = text_range_to_range(
                TextRange::empty(action.import_offset),
                &source,
                &index,
                snapshot.encoding,
            ) else {
                continue;
            };
            let edit = TextDocumentEdit::new(
                OptionalVersionedTextDocumentIdentifier::new(
                    snapshot.version,
                    params.text_document.clone(),
                ),
                vec![
                    TextEdit::new(edit_range, action.new_text).into(),
                    TextEdit::new(import_range, action.import_text).into(),
                ],
            );
            actions.push(
                CodeAction::new(
                    action.title,
                    Some(CodeActionKind::QuickFix),
                    Some(vec![diagnostic]),
                    None,
                    None,
                    Some(WorkspaceEdit::new(None, Some(vec![edit.into()]), None)),
                    None,
                    None,
                    None,
                )
                .into(),
            );
        }
        Ok(Some(actions))
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        snapshot
            .analysis
            .as_ref()
            .map(PreparedSourceAnalysis::response_version)
    }
}
