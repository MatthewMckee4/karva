use std::collections::BTreeMap;

use camino::{Utf8Path, Utf8PathBuf};
use karva_collector::{CollectedModule, CollectionSettings, collect_source};

use crate::{SourceAnalysis, SourceAnalysisSettings, SourceDocument, analyze_collected_source};

/// Immutable syntax index for the Python sources in one Karva workspace.
///
/// The index owns one collected module per source path. It does not read from
/// the filesystem or observe later changes; callers build a new index when
/// their workspace snapshot changes. This keeps editor analysis deterministic
/// and lets a language-server session publish one coherent source snapshot to
/// its request handlers.
#[derive(Debug)]
pub struct WorkspaceSourceIndex {
    project_root: Utf8PathBuf,
    modules: BTreeMap<Utf8PathBuf, CollectedModule>,
    settings: SourceAnalysisSettings,
}

impl WorkspaceSourceIndex {
    /// Builds an index from already-collected modules.
    ///
    /// When a path occurs more than once, the last module wins. This permits a
    /// caller to layer an unsaved document over a disk snapshot without
    /// mutating an existing index.
    pub fn from_modules(
        project_root: Utf8PathBuf,
        settings: SourceAnalysisSettings,
        modules: impl IntoIterator<Item = CollectedModule>,
    ) -> Self {
        let modules = modules
            .into_iter()
            .map(|module| (module.path.path().clone(), module))
            .collect();
        Self {
            project_root,
            modules,
            settings,
        }
    }

    /// Collects each source document once and builds an immutable index.
    ///
    /// Returns `None` when any source cannot be collected beneath the project
    /// root. A later document with the same path replaces an earlier one
    /// before collection, so duplicate inputs never cause duplicate parsing
    /// work.
    pub fn from_documents(
        project_root: Utf8PathBuf,
        documents: impl IntoIterator<Item = SourceDocument>,
        settings: SourceAnalysisSettings,
    ) -> Option<Self> {
        let collection_settings = CollectionSettings {
            python_version: settings.python_version,
            test_function_prefix: &settings.test_function_prefix,
            respect_ignore_files: true,
            collect_fixtures: true,
            collect_doctests: false,
        };
        let documents = documents
            .into_iter()
            .map(SourceDocument::into_parts)
            .collect::<BTreeMap<_, _>>();
        let modules = documents
            .into_iter()
            .map(|(path, source_text)| {
                collect_source(&path, &project_root, source_text, &collection_settings, &[])
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self::from_modules(project_root, settings, modules))
    }

    /// Returns the workspace root used for module collection.
    pub fn project_root(&self) -> &Utf8Path {
        &self.project_root
    }

    /// Returns the collected module for `path`, if the snapshot contains it.
    pub fn module(&self, path: &Utf8Path) -> Option<&CollectedModule> {
        self.modules.get(path)
    }

    /// Returns source paths in deterministic lexical order.
    pub fn paths(&self) -> impl Iterator<Item = &Utf8Path> {
        self.modules.keys().map(Utf8PathBuf::as_path)
    }

    /// Analyzes one indexed source with visible ancestor `conftest.py` files.
    ///
    /// Ancestors are passed to the source analyzer from the project root
    /// toward the nearest package, matching runtime fixture precedence. The
    /// current module is cloned because [`SourceAnalysis`] owns its parsed
    /// syntax tree; parent modules remain borrowed from the immutable index.
    pub fn analyze(&self, path: &Utf8Path) -> Option<SourceAnalysis> {
        let current = self.modules.get(path)?.clone();
        let parents = self.parent_modules(path);
        Some(analyze_collected_source(current, &parents, &self.settings))
    }

    fn parent_modules(&self, path: &Utf8Path) -> Vec<&CollectedModule> {
        let mut directory = path.parent();
        let mut parents = Vec::new();
        while let Some(current) = directory {
            if !current.starts_with(&self.project_root) {
                break;
            }
            let conftest = current.join("conftest.py");
            if conftest != path
                && let Some(module) = self.modules.get(&conftest)
            {
                parents.push(module);
            }
            if current == self.project_root {
                break;
            }
            directory = current.parent();
        }
        parents.reverse();
        parents
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8Path;
    use ruff_python_ast::PythonVersion;

    use super::*;
    use crate::{FixtureResolution, SourceAnalysisSettings};

    fn settings() -> SourceAnalysisSettings {
        SourceAnalysisSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test".to_owned(),
            try_import_fixtures: false,
        }
    }

    fn document(path: &str, source: &str) -> SourceDocument {
        SourceDocument::new(path.into(), source.to_owned())
    }

    #[test]
    fn paths_are_sorted_and_duplicate_documents_are_collected_once() {
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [
                document("/project/tests/z_test.py", "def test_z(): pass\n"),
                document("/project/tests/a_test.py", "def test_a(): pass\n"),
                document("/project/tests/a_test.py", "def test_replacement(): pass\n"),
            ],
            settings(),
        )
        .expect("documents should index");

        assert_eq!(
            index.paths().collect::<Vec<_>>(),
            [
                Utf8Path::new("/project/tests/a_test.py"),
                Utf8Path::new("/project/tests/z_test.py"),
            ]
        );
        assert_eq!(
            index
                .module(Utf8Path::new("/project/tests/a_test.py"))
                .map(|module| module.source_text.as_str()),
            Some("def test_replacement(): pass\n")
        );
    }

    #[test]
    fn rejects_documents_outside_project_root() {
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [document(
                "/other/test_example.py",
                "def test_example(): pass\n",
            )],
            settings(),
        );

        assert!(index.is_none());
    }

    #[test]
    fn analysis_uses_root_to_nearest_conftest_precedence() {
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [
                document(
                    "/project/conftest.py",
                    "from karva import fixture\n\n@fixture\ndef database(): pass\n",
                ),
                document(
                    "/project/pkg/conftest.py",
                    "from karva import fixture\n\n@fixture\ndef database(): pass\n",
                ),
                document(
                    "/project/pkg/test_example.py",
                    "from karva import fixture\n\n@fixture\ndef local(database): pass\n",
                ),
            ],
            settings(),
        )
        .expect("documents should index");

        let analysis = index
            .analyze(Utf8Path::new("/project/pkg/test_example.py"))
            .expect("indexed source should analyze");
        assert!(matches!(
            &analysis.fixtures[0].dependencies[0].resolution,
            FixtureResolution::Resolved(id) if id.path == "/project/pkg/conftest.py"
        ));
    }

    #[test]
    fn analysis_does_not_use_sibling_conftest() {
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [
                document(
                    "/project/other/conftest.py",
                    "from karva import fixture\n\n@fixture\ndef database(): pass\n",
                ),
                document(
                    "/project/pkg/test_example.py",
                    "def test_example(database): pass\n",
                ),
            ],
            settings(),
        )
        .expect("documents should index");

        let analysis = index
            .analyze(Utf8Path::new("/project/pkg/test_example.py"))
            .expect("indexed source should analyze");
        assert!(matches!(
            &analysis.diagnostics[0].code,
            crate::DiagnosticCode::MissingFixture
        ));
    }

    #[test]
    fn conftest_is_not_its_own_parent() {
        let path = Utf8Path::new("/project/pkg/conftest.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [document(
                path.as_str(),
                "from karva import fixture\n\n@fixture\ndef database(): pass\n",
            )],
            settings(),
        )
        .expect("documents should index");

        let analysis = index.analyze(path).expect("indexed source should analyze");

        assert!(index.parent_modules(path).is_empty());
        assert!(analysis.diagnostics.is_empty());
        assert_eq!(analysis.visible_fixtures.len(), 1);
    }
}
