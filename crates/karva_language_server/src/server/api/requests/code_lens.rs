//! Fully resolved editor commands for collected tests; this endpoint never executes them.

use karva_ide::source_tests;
use lsp_types::{CodeLens, CodeLensParams, CodeLensRequest, Command};
use ruff_source_file::LineIndex;
use serde_json::json;

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::text_range_to_range;
use crate::session::client::Client;
use crate::session::{
    DocumentSnapshotVersion, PreparedSourceAnalysis, RequestCancellationToken, Session,
};
use crate::workspace::uri_to_path;

pub(in crate::server::api) struct CodeLenses;

pub(in crate::server::api) struct CodeLensesSnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    encoding: PositionEncoding,
    profile: Option<String>,
}

impl RequestHandler for CodeLenses {
    type RequestType = CodeLensRequest;
}

impl BackgroundRequestHandler for CodeLenses {
    type Snapshot = CodeLensesSnapshot;

    fn prepare(session: &mut Session, params: &CodeLensParams) -> anyhow::Result<Self::Snapshot> {
        Ok(CodeLensesSnapshot {
            analysis: session.prepare_source_analysis(&params.text_document.uri)?,
            encoding: session.position_encoding(),
            profile: session.configuration_profile().map(str::to_owned),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        _params: CodeLensParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<Vec<CodeLens>>> {
        let Some(prepared) = snapshot.analysis else {
            return Ok(None);
        };
        let uri = prepared.response_version().uri;
        let path = uri_to_path(&uri)?;
        let source = prepared.source_text().to_owned();
        let index = LineIndex::from_source_text(&source);
        let Some(analysis) = prepared.analyze(cancellation)? else {
            return Ok(None);
        };
        let root = analysis.source_index.project_root();
        let relative = path.strip_prefix(root)?;
        // CLI selectors use forward slashes on every platform, with arguments
        // passed separately rather than assembled into shell command text.
        let file_selector = relative.as_str().replace('\\', "/");
        let mut lenses = Vec::new();
        for test in source_tests(&analysis.analysis) {
            if cancellation.is_cancelled() {
                return Ok(None);
            }
            let Some(range) = text_range_to_range(test.range, &source, &index, snapshot.encoding)
            else {
                continue;
            };
            for (title, selector) in [
                ("Run test", format!("{file_selector}::{}", test.name)),
                ("Run file", file_selector.clone()),
            ] {
                let mut args = vec![
                    "run".to_owned(),
                    "karva".to_owned(),
                    "test".to_owned(),
                    selector,
                ];
                if let Some(profile) = &snapshot.profile {
                    args.push(format!("--profile={profile}"));
                }
                lenses.push(CodeLens::new(
                    range,
                    Some(Command::new(
                        title.to_owned(),
                        None,
                        "karva.runTests".to_owned(),
                        Some(vec![json!({"cwd": root, "program": "uv", "args": args})]),
                    )),
                    None,
                ));
            }
            lenses.push(CodeLens::new(
                range,
                Some(Command::new(
                    "Copy test ID".to_owned(),
                    None,
                    "karva.copyTestId".to_owned(),
                    Some(vec![json!(test.qualified_id)]),
                )),
                None,
            ));
        }
        Ok(Some(lenses))
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        snapshot
            .analysis
            .as_ref()
            .map(PreparedSourceAnalysis::response_version)
    }
}
