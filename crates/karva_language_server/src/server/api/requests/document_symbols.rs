//! Source symbols for one Python document.

use karva_ide::{SourceSymbol, SourceSymbolKind, source_symbols};
use lsp_types::{
    BaseSymbolInformation, DocumentSymbol, DocumentSymbolParams, DocumentSymbolRequest,
    DocumentSymbolResponse, Location, SymbolInformation, SymbolKind,
};
use ruff_source_file::LineIndex;

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::text_range_to_range;
use crate::session::client::Client;
use crate::session::{
    DocumentSnapshotVersion, PreparedSourceAnalysis, RequestCancellationToken, Session,
};

pub(in crate::server::api) struct DocumentSymbols;

pub(in crate::server::api) struct DocumentSymbolsSnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    position_encoding: PositionEncoding,
    hierarchical: bool,
}

impl RequestHandler for DocumentSymbols {
    type RequestType = DocumentSymbolRequest;
}

impl BackgroundRequestHandler for DocumentSymbols {
    type Snapshot = DocumentSymbolsSnapshot;

    fn prepare(
        session: &mut Session,
        params: &DocumentSymbolParams,
    ) -> anyhow::Result<Self::Snapshot> {
        Ok(DocumentSymbolsSnapshot {
            analysis: session.prepare_source_analysis(&params.text_document.uri)?,
            position_encoding: session.position_encoding(),
            hierarchical: session.supports_hierarchical_document_symbols(),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        _params: DocumentSymbolParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<DocumentSymbolResponse>> {
        let Some(prepared) = snapshot.analysis else {
            return Ok(None);
        };
        let uri = prepared.response_version().uri;
        let source_text = prepared.source_text().to_owned();
        let line_index = LineIndex::from_source_text(&source_text);
        let Some(analysis) = prepared.analyze(cancellation)? else {
            return Ok(None);
        };
        let symbols = source_symbols(&analysis.analysis);
        if symbols.is_empty() {
            return Ok(None);
        }

        if snapshot.hierarchical {
            Ok(Some(DocumentSymbolResponse::DocumentSymbolList(
                symbols
                    .iter()
                    .filter_map(|symbol| {
                        convert_document_symbol(
                            symbol,
                            &source_text,
                            &line_index,
                            snapshot.position_encoding,
                        )
                    })
                    .collect(),
            )))
        } else {
            Ok(Some(DocumentSymbolResponse::SymbolInformationList(
                symbols
                    .iter()
                    .filter_map(|symbol| {
                        convert_symbol_information(
                            symbol,
                            uri.clone(),
                            &source_text,
                            &line_index,
                            snapshot.position_encoding,
                        )
                    })
                    .collect(),
            )))
        }
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        snapshot
            .analysis
            .as_ref()
            .map(PreparedSourceAnalysis::response_version)
    }
}

fn symbol_kind(kind: SourceSymbolKind) -> SymbolKind {
    match kind {
        SourceSymbolKind::Test | SourceSymbolKind::Fixture => SymbolKind::Function,
    }
}

fn convert_document_symbol(
    symbol: &SourceSymbol,
    source_text: &str,
    line_index: &LineIndex,
    position_encoding: PositionEncoding,
) -> Option<DocumentSymbol> {
    Some(DocumentSymbol {
        name: symbol.name.clone(),
        detail: symbol.detail.clone(),
        kind: symbol_kind(symbol.kind),
        tags: None,
        #[allow(deprecated)]
        deprecated: None,
        range: text_range_to_range(symbol.range, source_text, line_index, position_encoding)?,
        selection_range: text_range_to_range(
            symbol.selection_range,
            source_text,
            line_index,
            position_encoding,
        )?,
        children: None,
    })
}

fn convert_symbol_information(
    symbol: &SourceSymbol,
    uri: lsp_types::Uri,
    source_text: &str,
    line_index: &LineIndex,
    position_encoding: PositionEncoding,
) -> Option<SymbolInformation> {
    Some(SymbolInformation {
        #[allow(deprecated)]
        deprecated: None,
        location: Location::new(
            uri,
            text_range_to_range(symbol.range, source_text, line_index, position_encoding)?,
        ),
        base_symbol_information: BaseSymbolInformation {
            name: symbol.name.clone(),
            kind: symbol_kind(symbol.kind),
            tags: None,
            container_name: None,
        },
    })
}
