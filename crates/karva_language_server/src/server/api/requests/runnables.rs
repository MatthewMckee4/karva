//! Native test run targets for editors that support LSP runnables.

use std::collections::HashMap;

use camino::Utf8PathBuf;
use karva_ide::{SourceTestTarget, source_test_targets};
use lsp_types::{
    LocationLink, LspRequestMethod, MessageDirection, Position, TextDocumentIdentifier,
};
use ruff_source_file::LineIndex;
use ruff_text_size::{TextRange, TextSize};
use serde::{Deserialize, Serialize};

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::{position_to_text_size, text_range_to_range};
use crate::session::client::Client;
use crate::session::{
    DocumentSnapshotVersion, PreparedSourceAnalysis, RequestCancellationToken, Session,
};

#[derive(Debug)]
pub(in crate::server::api) enum RunnablesRequest {}

impl lsp_types::Request for RunnablesRequest {
    type Params = RunnablesParams;
    type Result = Vec<Runnable>;

    const METHOD: LspRequestMethod<'static> = LspRequestMethod::new("experimental/runnables");
    const MESSAGE_DIRECTION: MessageDirection = MessageDirection::ClientToServer;
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::server::api) struct RunnablesParams {
    text_document: TextDocumentIdentifier,

    #[serde(default)]
    position: Option<Position>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::server::api) struct Runnable {
    label: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<LocationLink>,

    #[serde(flatten)]
    args: RunnableArgs,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", content = "args", rename_all = "lowercase")]
enum RunnableArgs {
    Shell(ShellRunnableArgs),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShellRunnableArgs {
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    environment: HashMap<String, String>,

    cwd: Utf8PathBuf,

    program: String,

    args: Vec<String>,
}

pub(in crate::server::api) struct Runnables;

pub(in crate::server::api) struct RunnablesSnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    position_encoding: PositionEncoding,
}

impl RequestHandler for Runnables {
    type RequestType = RunnablesRequest;
}

impl BackgroundRequestHandler for Runnables {
    type Snapshot = RunnablesSnapshot;

    fn prepare(session: &mut Session, params: &RunnablesParams) -> anyhow::Result<Self::Snapshot> {
        Ok(RunnablesSnapshot {
            analysis: session.prepare_open_document_source_analysis(&params.text_document.uri)?,
            position_encoding: session.position_encoding(),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: RunnablesParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Vec<Runnable>> {
        let Some(prepared) = snapshot.analysis else {
            return Ok(Vec::new());
        };
        let source_text = prepared.source_text().to_owned();
        let line_index = LineIndex::from_source_text(&source_text);
        let requested_offset = params.position.map(|position| {
            position_to_text_size(
                position,
                &source_text,
                &line_index,
                snapshot.position_encoding,
            )
        });
        let uri = prepared.response_version().uri;
        let doctest_modules = prepared.doctest_modules();
        let project_root = prepared.project_root().to_path_buf();
        let current_path = prepared.current_path().to_path_buf();
        let profile = prepared.profile().map(str::to_owned);
        let Some(analysis) = prepared.analyze(cancellation)? else {
            return Ok(Vec::new());
        };
        let relative_path = current_path
            .strip_prefix(&project_root)
            .unwrap_or(&current_path);
        let path_selector = relative_path.as_str().replace('\\', "/");

        let mut runnables = if requested_offset.is_none()
            && let Some(document_location) = location_for_range(
                TextRange::empty(TextSize::new(0)),
                TextRange::empty(TextSize::new(0)),
                uri.clone(),
                &source_text,
                &line_index,
                snapshot.position_encoding,
            ) {
            vec![
                shell_runnable_at(
                    "Run Karva project",
                    project_root.clone(),
                    profile.as_deref(),
                    None,
                    document_location.clone(),
                ),
                shell_runnable_at(
                    "Run Karva file",
                    project_root.clone(),
                    profile.as_deref(),
                    Some(path_selector.clone()),
                    document_location,
                ),
            ]
        } else {
            Vec::new()
        };

        for target in source_test_targets(&analysis.analysis, doctest_modules) {
            if requested_offset.is_some_and(|offset| !target.range.contains(offset)) {
                continue;
            }
            let Some(location) = target_location(
                &target,
                uri.clone(),
                &source_text,
                &line_index,
                snapshot.position_encoding,
            ) else {
                continue;
            };
            let selector = format!("{path_selector}::{}", target.name);
            runnables.push(shell_runnable_at(
                format!("Run {}", target.name),
                project_root.clone(),
                profile.as_deref(),
                Some(selector),
                location,
            ));
        }

        Ok(runnables)
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        snapshot
            .analysis
            .as_ref()
            .map(PreparedSourceAnalysis::response_version)
    }
}

fn shell_runnable(
    label: impl Into<String>,
    cwd: Utf8PathBuf,
    profile: Option<&str>,
    selector: Option<String>,
) -> Runnable {
    let mut args = vec!["run".to_owned(), "karva".to_owned(), "test".to_owned()];
    if let Some(profile) = profile {
        args.extend(["--profile".to_owned(), profile.to_owned()]);
    }
    args.extend(selector);
    Runnable {
        label: label.into(),
        location: None,
        args: RunnableArgs::Shell(ShellRunnableArgs {
            environment: HashMap::new(),
            cwd,
            program: "uv".to_owned(),
            args,
        }),
    }
}

fn shell_runnable_at(
    label: impl Into<String>,
    cwd: Utf8PathBuf,
    profile: Option<&str>,
    selector: Option<String>,
    location: LocationLink,
) -> Runnable {
    Runnable {
        location: Some(location),
        ..shell_runnable(label, cwd, profile, selector)
    }
}

fn target_location(
    target: &SourceTestTarget,
    uri: lsp_types::Uri,
    source_text: &str,
    line_index: &LineIndex,
    position_encoding: PositionEncoding,
) -> Option<LocationLink> {
    let target_range = TextRange::new(target.selection_range.start(), target.range.end());
    location_for_range(
        target_range,
        target.selection_range,
        uri,
        source_text,
        line_index,
        position_encoding,
    )
}

fn location_for_range(
    range: TextRange,
    selection_range: TextRange,
    uri: lsp_types::Uri,
    source_text: &str,
    line_index: &LineIndex,
    position_encoding: PositionEncoding,
) -> Option<LocationLink> {
    Some(LocationLink {
        origin_selection_range: None,
        target_uri: uri,
        target_range: text_range_to_range(range, source_text, line_index, position_encoding)?,
        target_selection_range: text_range_to_range(
            selection_range,
            source_text,
            line_index,
            position_encoding,
        )?,
    })
}
