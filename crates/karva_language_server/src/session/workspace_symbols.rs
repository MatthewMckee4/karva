//! Owned workspace-symbol inputs; all walking, reads, and analysis run on a worker.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use camino::Utf8PathBuf;
use karva_ide::{SourceAnalysisSettings, SourceSymbol, analyze_source, source_symbols};
use lsp_types::Uri;
use once_cell::sync::OnceCell;

use super::RequestCancellationToken;
use super::source_index::{SourceIndexError, discover_directory, reject_symlink_root};
use crate::workspace::Workspaces;

/// Resolved source and symbol locations for one workspace notification generation.
#[derive(Debug)]
pub(super) struct IndexedWorkspaceSymbols {
    path: Utf8PathBuf,
    source: String,
    symbols: Vec<SourceSymbol>,
}

pub(super) type WorkspaceSymbolCache = Arc<OnceCell<Vec<IndexedWorkspaceSymbols>>>;

/// A coherent set of workspace roots and unsaved Python source overlays.
pub struct PreparedWorkspaceSymbols {
    workspaces: Workspaces,
    overlays: BTreeMap<Utf8PathBuf, Arc<str>>,
    /// Reused only while the session's notification generation is current.
    cache: WorkspaceSymbolCache,
}

impl PreparedWorkspaceSymbols {
    pub(super) fn new(
        workspaces: Workspaces,
        overlays: BTreeMap<Utf8PathBuf, Arc<str>>,
        cache: WorkspaceSymbolCache,
    ) -> Self {
        Self {
            workspaces,
            overlays,
            cache,
        }
    }

    /// Visits each source once, using its nearest project configuration and overlay.
    pub fn visit(
        self,
        cancellation: &RequestCancellationToken,
        mut visit: impl FnMut(&Utf8PathBuf, &str, Vec<SourceSymbol>),
    ) -> anyhow::Result<()> {
        if cancellation.is_cancelled() {
            return Err(SourceIndexError::Cancelled.into());
        }
        let cache = Arc::clone(&self.cache);
        let modules = cache.get_or_try_init(|| self.collect(cancellation))?;
        for module in modules {
            if cancellation.is_cancelled() {
                return Err(SourceIndexError::Cancelled.into());
            }
            visit(&module.path, &module.source, module.symbols.clone());
        }
        Ok(())
    }

    /// Resolves nested project settings before storing a coherent symbol snapshot.
    fn collect(
        self,
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<Vec<IndexedWorkspaceSymbols>> {
        if cancellation.is_cancelled() {
            return Err(SourceIndexError::Cancelled.into());
        }
        let mut paths = BTreeSet::new();
        for root in self.workspaces.roots() {
            if cancellation.is_cancelled() {
                return Err(SourceIndexError::Cancelled.into());
            }
            reject_symlink_root(root)?;
            let uri = Uri::from_file_path(root.join("__karva_workspace__.py").as_std_path())
                .map_err(|()| anyhow::anyhow!("workspace root is not a file URI: {root}"))?;
            let project = self
                .workspaces
                .prepare_project_discovery(&uri)?
                .discover()?;
            discover_directory(
                root,
                project.settings().src().respect_ignore_files,
                cancellation,
                &mut paths,
            )?;
        }
        paths.extend(
            self.overlays
                .keys()
                .filter(|path| {
                    path.extension() == Some("py")
                        && self.workspaces.roots().any(|root| path.starts_with(root))
                })
                .cloned(),
        );
        let mut modules = Vec::with_capacity(paths.len());
        for path in paths {
            if cancellation.is_cancelled() {
                return Err(SourceIndexError::Cancelled.into());
            }
            let uri = Uri::from_file_path(path.as_std_path())
                .map_err(|()| anyhow::anyhow!("source path is not a file URI: {path}"))?;
            let project = self
                .workspaces
                .prepare_project_discovery(&uri)?
                .discover()?;
            let source = if let Some(source) = self.overlays.get(&path) {
                source.to_string()
            } else {
                std::fs::read_to_string(&path).map_err(|source| SourceIndexError::ReadSource {
                    path: path.clone(),
                    source,
                })?
            };
            let settings = SourceAnalysisSettings {
                python_version: project.metadata().python_version(),
                test_function_prefix: project.settings().test().test_function_prefix.clone(),
                try_import_fixtures: project.settings().test().try_import_fixtures,
            };
            if let Some(analysis) = analyze_source(&path, project.cwd(), source.clone(), &settings)
            {
                modules.push(IndexedWorkspaceSymbols {
                    path,
                    source,
                    symbols: source_symbols(&analysis),
                });
            }
        }
        Ok(modules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruff_python_ast::PythonVersion;

    #[rstest::rstest]
    fn cancelled_search_never_visits_sources(
        #[values(false, true)] warm: bool,
    ) -> anyhow::Result<()> {
        let token = RequestCancellationToken::default();
        token.cancel();
        let workspaces = Workspaces::new(Vec::new(), PythonVersion::PY312, None)?;
        let cache = WorkspaceSymbolCache::default();
        if warm {
            PreparedWorkspaceSymbols::new(workspaces.clone(), BTreeMap::new(), Arc::clone(&cache))
                .visit(&RequestCancellationToken::default(), |_, _, _| {})?;
        }
        let mut visited = false;
        let result = PreparedWorkspaceSymbols::new(workspaces, BTreeMap::new(), cache)
            .visit(&token, |_, _, _| visited = true);
        assert!(result.is_err());
        assert!(!visited);
        Ok(())
    }
}
