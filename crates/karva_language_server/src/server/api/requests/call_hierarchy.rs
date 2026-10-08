//! Fixture graph navigation from coherent project source snapshots.

use karva_ide::{
    FixtureHierarchyCall, FixtureHierarchyItem, SourceSymbolKind, WorkspaceSourceIndex,
    fixture_hierarchy_items, fixture_hierarchy_provider, fixture_incoming_calls,
    fixture_outgoing_calls, fixture_target,
};
use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsParams, CallHierarchyIncomingCallsRequest,
    CallHierarchyItem, CallHierarchyOutgoingCall, CallHierarchyOutgoingCallsParams,
    CallHierarchyOutgoingCallsRequest, CallHierarchyPrepareParams, CallHierarchyPrepareRequest,
    SymbolKind, Uri,
};
use ruff_source_file::LineIndex;

use super::super::traits::{BackgroundRequestHandler, RequestHandler};
use crate::PositionEncoding;
use crate::document::{position_to_text_size, text_range_to_range};
use crate::session::client::Client;
use crate::session::{
    DocumentSnapshotVersion, PreparedSourceAnalysis, RequestCancellationToken, Session,
};
use crate::workspace::uri_to_path;

pub(in crate::server::api) struct PrepareCallHierarchy;
pub(in crate::server::api) struct IncomingCalls;
pub(in crate::server::api) struct OutgoingCalls;

/// Retains an open origin document while navigating providers in unopened files.
pub(in crate::server::api) struct HierarchySnapshot {
    analysis: Option<PreparedSourceAnalysis>,
    encoding: PositionEncoding,
    origin: Uri,
}

impl HierarchySnapshot {
    fn prepare(session: &Session, origin: Uri) -> anyhow::Result<Self> {
        Ok(Self {
            analysis: if session.document(&origin).is_some() {
                session.prepare_project_source_analysis(&origin)?
            } else {
                None
            },
            encoding: session.position_encoding(),
            origin,
        })
    }

    fn for_item(session: &Session, item: &CallHierarchyItem) -> anyhow::Result<Self> {
        let origin = item
            .data
            .as_ref()
            .and_then(|data| serde_json::from_value::<Uri>(data.clone()).ok())
            .unwrap_or_else(|| item.uri.clone());
        Self::prepare(session, origin)
    }

    fn version(&self) -> Option<DocumentSnapshotVersion> {
        self.analysis
            .as_ref()
            .map(PreparedSourceAnalysis::response_version)
    }
}

impl RequestHandler for PrepareCallHierarchy {
    type RequestType = CallHierarchyPrepareRequest;
}
impl BackgroundRequestHandler for PrepareCallHierarchy {
    type Snapshot = HierarchySnapshot;

    fn prepare(
        session: &mut Session,
        params: &CallHierarchyPrepareParams,
    ) -> anyhow::Result<Self::Snapshot> {
        HierarchySnapshot::prepare(
            session,
            params
                .text_document_position_params
                .text_document
                .uri
                .clone(),
        )
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: CallHierarchyPrepareParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<Vec<CallHierarchyItem>>> {
        let Some(prepared) = snapshot.analysis else {
            return Ok(None);
        };
        let source = prepared.source_text().to_owned();
        let offset = position_to_text_size(
            params.text_document_position_params.position,
            &source,
            &LineIndex::from_source_text(&source),
            snapshot.encoding,
        );
        let Some(analysis) = prepared.analyze(cancellation)? else {
            return Ok(None);
        };
        let Some(target) = fixture_target(&analysis.analysis, offset) else {
            return Ok(None);
        };
        let item = fixture_hierarchy_provider(&analysis.source_index, &target).and_then(|item| {
            protocol_item(
                &analysis.source_index,
                &item,
                snapshot.encoding,
                &snapshot.origin,
            )
        });
        Ok(item.map(|item| vec![item]))
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        snapshot.version()
    }
}

impl RequestHandler for IncomingCalls {
    type RequestType = CallHierarchyIncomingCallsRequest;
}
impl BackgroundRequestHandler for IncomingCalls {
    type Snapshot = HierarchySnapshot;

    fn prepare(
        session: &mut Session,
        params: &CallHierarchyIncomingCallsParams,
    ) -> anyhow::Result<Self::Snapshot> {
        HierarchySnapshot::for_item(session, &params.item)
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: CallHierarchyIncomingCallsParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<Vec<CallHierarchyIncomingCall>>> {
        Ok(
            calls(snapshot, &params.item, cancellation, true)?.map(|calls| {
                calls
                    .into_iter()
                    .map(|(item, ranges)| CallHierarchyIncomingCall::new(item, ranges))
                    .collect()
            }),
        )
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        snapshot.version()
    }
}

impl RequestHandler for OutgoingCalls {
    type RequestType = CallHierarchyOutgoingCallsRequest;
}
impl BackgroundRequestHandler for OutgoingCalls {
    type Snapshot = HierarchySnapshot;

    fn prepare(
        session: &mut Session,
        params: &CallHierarchyOutgoingCallsParams,
    ) -> anyhow::Result<Self::Snapshot> {
        HierarchySnapshot::for_item(session, &params.item)
    }

    fn run(
        snapshot: Self::Snapshot,
        _client: &Client,
        params: CallHierarchyOutgoingCallsParams,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        Ok(
            calls(snapshot, &params.item, cancellation, false)?.map(|calls| {
                calls
                    .into_iter()
                    .map(|(item, ranges)| CallHierarchyOutgoingCall::new(item, ranges))
                    .collect()
            }),
        )
    }

    fn document_version(snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        snapshot.version()
    }
}

type ProtocolCalls = Vec<(CallHierarchyItem, Vec<lsp_types::Range>)>;

/// Revalidates the client item against current syntax before resolving fresh edges.
fn calls(
    snapshot: HierarchySnapshot,
    item: &CallHierarchyItem,
    cancellation: &RequestCancellationToken,
    incoming: bool,
) -> anyhow::Result<Option<ProtocolCalls>> {
    let Some(prepared) = snapshot.analysis else {
        return Ok(None);
    };
    let Some(analysis) = prepared.analyze(cancellation)? else {
        return Ok(None);
    };
    let index = &analysis.source_index;
    let path = uri_to_path(&item.uri)?;
    let Some(current) = index.analyze(&path) else {
        return Ok(Some(Vec::new()));
    };
    let Some(selected) = fixture_hierarchy_items(&current)
        .into_iter()
        .find(|candidate| {
            protocol_item(index, candidate, snapshot.encoding, &snapshot.origin).is_some_and(
                |candidate| {
                    candidate.name == item.name
                        && candidate.range == item.range
                        && candidate.selection_range == item.selection_range
                },
            )
        })
    else {
        return Ok(Some(Vec::new()));
    };
    let edges = if incoming {
        fixture_target(&current, selected.selection_range.start())
            .map(|target| fixture_incoming_calls(index, &target))
            .unwrap_or_default()
    } else {
        fixture_outgoing_calls(index, &selected)
    };
    let mut result = Vec::new();
    for FixtureHierarchyCall {
        item: endpoint,
        ranges,
    } in edges
    {
        if cancellation.is_cancelled() {
            return Ok(None);
        }
        let consumer_path = if incoming {
            &endpoint.path
        } else {
            &selected.path
        };
        let Some(module) = index.module(consumer_path) else {
            continue;
        };
        let source = &module.source_text;
        let lines = LineIndex::from_source_text(source);
        let ranges = ranges
            .into_iter()
            .filter_map(|range| text_range_to_range(range, source, &lines, snapshot.encoding))
            .collect();
        if let Some(endpoint) = protocol_item(index, &endpoint, snapshot.encoding, &snapshot.origin)
        {
            result.push((endpoint, ranges));
        }
    }
    Ok(Some(result))
}

fn protocol_item(
    index: &WorkspaceSourceIndex,
    item: &FixtureHierarchyItem,
    encoding: PositionEncoding,
    origin: &Uri,
) -> Option<CallHierarchyItem> {
    let source = &index.module(&item.path)?.source_text;
    let lines = LineIndex::from_source_text(source);
    Some(CallHierarchyItem::new(
        if item.kind == SourceSymbolKind::Fixture {
            format!("{} (fixture)", item.name)
        } else {
            item.name.clone()
        },
        SymbolKind::Function,
        None,
        None,
        Uri::from_file_path(item.path.as_std_path()).ok()?,
        text_range_to_range(item.range, source, &lines, encoding)?,
        text_range_to_range(item.selection_range, source, &lines, encoding)?,
        Some(serde_json::to_value(origin).ok()?),
    ))
}
