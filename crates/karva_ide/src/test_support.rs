//! Source document builders used by editor feature tests.

#![expect(
    clippy::redundant_pub_crate,
    reason = "test helpers are restricted to sibling test modules rather than a public API"
)]

use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use karva_collector::{CollectedModule, CollectionSettings, collect_source};

use crate::{SourceAnalysis, SourceAnalysisSettings, WorkspaceSourceIndex};

#[derive(Clone)]
pub(super) struct SourceDocument {
    path: Utf8PathBuf,
    source_text: String,
}

impl SourceDocument {
    pub(super) fn new(path: Utf8PathBuf, source_text: String) -> Self {
        Self { path, source_text }
    }
}

pub(super) fn analyze_source_with_parents(
    current: SourceDocument,
    parents: impl IntoIterator<Item = SourceDocument>,
    project_root: &Utf8Path,
    settings: &SourceAnalysisSettings,
) -> Option<SourceAnalysis> {
    let path = current.path.clone();
    source_index(
        project_root.to_owned(),
        parents.into_iter().chain([current]),
        settings.clone(),
    )?
    .analyze(&path)
}

pub(super) fn source_index(
    project_root: Utf8PathBuf,
    documents: impl IntoIterator<Item = SourceDocument>,
    settings: SourceAnalysisSettings,
) -> Option<WorkspaceSourceIndex> {
    let collection_settings = CollectionSettings {
        python_version: settings.python_version,
        test_function_prefix: &settings.test_function_prefix,
        respect_ignore_files: true,
        collect_fixtures: true,
        collect_doctests: false,
    };
    let documents = documents
        .into_iter()
        .map(|document| (document.path, document.source_text))
        .collect::<std::collections::BTreeMap<_, _>>();
    let modules = documents
        .into_iter()
        .map(|(path, source_text)| {
            collect_source(&path, &project_root, source_text, &collection_settings, &[])
                .map(Arc::new)
        })
        .collect::<Option<Vec<_>>>()?;
    Some(WorkspaceSourceIndex::from_shared_modules(
        project_root,
        settings,
        modules,
    ))
}

pub(super) fn analyze_sources(
    current: CollectedModule,
    parents: &[CollectedModule],
    settings: &SourceAnalysisSettings,
) -> SourceAnalysis {
    let path = current.path.path().clone();
    let modules = parents.iter().cloned().chain([current]).map(Arc::new);
    WorkspaceSourceIndex::from_shared_modules(
        Utf8PathBuf::from("/project"),
        settings.clone(),
        modules,
    )
    .analyze(&path)
    .expect("indexed source")
}
