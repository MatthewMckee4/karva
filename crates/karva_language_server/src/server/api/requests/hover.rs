//! Framework-aware hover for Karva fixture references.

use karva_ide::{FixtureHover, hover_fixture};
use lsp_types::{HoverParams, HoverRequest, MarkupContent, MarkupKind};
use ruff_source_file::LineIndex;

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::{position_to_text_size, text_range_to_range};
use crate::session::client::Client;
use crate::session::{PreparedSourceAnalysis, Session};

pub(in crate::server::api) struct Hover;

pub(in crate::server::api) struct HoverSnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    position_encoding: PositionEncoding,
    markup_kind: MarkupKind,
}

impl RequestHandler for Hover {
    type RequestType = HoverRequest;
}

impl BackgroundRequestHandler for Hover {
    type Snapshot = HoverSnapshot;

    fn prepare(session: &mut Session, params: &HoverParams) -> anyhow::Result<Self::Snapshot> {
        let uri = &params.text_document_position_params.text_document.uri;
        Ok(HoverSnapshot {
            analysis: session.prepare_source_analysis(uri)?,
            position_encoding: session.position_encoding(),
            markup_kind: session.hover_markup_kind(),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: HoverParams,
    ) -> anyhow::Result<Option<lsp_types::Hover>> {
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
        let Some(hover) = hover_fixture(&analysis.analysis, offset) else {
            return Ok(None);
        };
        let range = text_range_to_range(
            hover.range,
            &source_text,
            &index,
            snapshot.position_encoding,
        );

        Ok(Some(lsp_types::Hover::new(
            MarkupContent::new(
                snapshot.markup_kind,
                render_hover(&hover, snapshot.markup_kind),
            )
            .into(),
            range,
        )))
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<(lsp_types::Uri, i32)> {
        snapshot
            .analysis
            .as_ref()
            .map(|analysis| (analysis.document_uri().clone(), analysis.document_version()))
    }
}

fn render_hover(hover: &FixtureHover, kind: MarkupKind) -> String {
    let scope = hover
        .scope
        .map_or("dynamic", karva_ide::FixtureScope::as_str);
    let auto_use = hover
        .auto_use
        .map_or_else(|| "dynamic".to_owned(), |value| value.to_string());
    let provider = hover
        .provider
        .as_ref()
        .map_or_else(|| "Karva built-in".to_owned(), |id| id.path.to_string());
    let dependencies = (!hover.dependencies.is_empty()).then(|| hover.dependencies.join(", "));

    match kind {
        MarkupKind::Markdown => {
            let mut content = markdown_python_block(&hover.source_signature);
            content.push_str("\n\nKarva fixture **");
            content.push_str(&escape_markdown(&hover.name));
            content.push_str("**\n\nScope: `");
            content.push_str(scope);
            content.push_str("`  \nAutouse: `");
            content.push_str(&auto_use);
            content.push_str("`  \nProvider: ");
            content.push_str(&escape_markdown(&provider));
            if let Some(dependencies) = dependencies {
                content.push_str("  \nDependencies: ");
                content.push_str(&escape_markdown(&dependencies));
            }
            if let Some(docstring) = &hover.docstring {
                content.push_str("\n\n");
                content.push_str(&escape_markdown(docstring));
            }
            content
        }
        MarkupKind::PlainText => {
            let mut content = format!(
                "{}\n\nKarva fixture: {}\nScope: {scope}\nAutouse: {auto_use}\nProvider: {provider}",
                hover.source_signature, hover.name
            );
            if let Some(dependencies) = dependencies {
                content.push_str("\nDependencies: ");
                content.push_str(&dependencies);
            }
            if let Some(docstring) = &hover.docstring {
                content.push_str("\n\n");
                content.push_str(docstring);
            }
            content
        }
    }
}

fn markdown_python_block(source: &str) -> String {
    let longest_run = source
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let fence = "`".repeat(longest_run.saturating_add(1).max(3));
    format!("{fence}python\n{source}\n{fence}")
}

fn escape_markdown(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(
            character,
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '<' | '>' | '#' | '|' | '!' | '~'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}
