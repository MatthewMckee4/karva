//! Workspace navigation for statically collected tests and fixture declarations.

use lsp_types::{
    BaseSymbolInformation, Location, SymbolInformation, SymbolKind, Uri, WorkspaceSymbolParams,
    WorkspaceSymbolRequest, WorkspaceSymbolResponse,
};
use ruff_source_file::LineIndex;

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::text_range_to_range;
use crate::session::client::Client;
use crate::session::{PreparedWorkspaceSymbols, RequestCancellationToken, Session};

pub(in crate::server::api) struct WorkspaceSymbols;

pub(in crate::server::api) struct WorkspaceSymbolsSnapshot {
    sources: PreparedWorkspaceSymbols,
    position_encoding: PositionEncoding,
}

impl RequestHandler for WorkspaceSymbols {
    type RequestType = WorkspaceSymbolRequest;
}

impl BackgroundRequestHandler for WorkspaceSymbols {
    type Snapshot = WorkspaceSymbolsSnapshot;

    fn prepare(
        session: &mut Session,
        _params: &WorkspaceSymbolParams,
    ) -> anyhow::Result<Self::Snapshot> {
        Ok(WorkspaceSymbolsSnapshot {
            sources: session.prepare_workspace_symbols(),
            position_encoding: session.position_encoding(),
        })
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: WorkspaceSymbolParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<WorkspaceSymbolResponse>> {
        let query = params.query.to_lowercase();
        let mut results = Vec::new();
        snapshot
            .sources
            .visit(cancellation, |path, source, symbols| {
                let Ok(uri) = Uri::from_file_path(path.as_std_path()) else {
                    return;
                };
                let index = LineIndex::from_source_text(source);
                for symbol in symbols {
                    if !matches_query(&symbol.name, &query) {
                        continue;
                    }
                    let Some(range) = text_range_to_range(
                        symbol.selection_range,
                        source,
                        &index,
                        snapshot.position_encoding,
                    ) else {
                        continue;
                    };
                    results.push(SymbolInformation {
                        #[expect(
                            deprecated,
                            reason = "legacy symbol response requires this field"
                        )]
                        deprecated: None,
                        location: Location::new(uri.clone(), range),
                        base_symbol_information: BaseSymbolInformation {
                            name: symbol.name,
                            kind: SymbolKind::Function,
                            tags: None,
                            container_name: path.file_name().map(str::to_owned),
                        },
                    });
                }
            })?;
        // Every location is fully resolved, so this response also supports clients
        // without workspace-symbol resolve capabilities.
        Ok(Some(WorkspaceSymbolResponse::SymbolInformationList(
            results,
        )))
    }
}

fn matches_query(name: &str, query: &str) -> bool {
    let name = name.to_lowercase();
    let mut chars = name.chars();
    query
        .chars()
        .all(|wanted| chars.any(|character| character == wanted))
}
