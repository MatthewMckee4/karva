use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use camino::{Utf8Path, Utf8PathBuf};
use karva_collector::CollectedModule;

#[cfg(test)]
use karva_collector::{CollectionSettings, collect_source, collect_source_with_module_name};

use crate::{SourceAnalysis, SourceAnalysisSettings, analyze_collected_source};

#[cfg(test)]
use crate::SourceDocument;

/// Immutable syntax index for the Python sources in one Karva workspace.
///
/// The index owns one collected module per source path. It does not read from
/// the filesystem or observe later changes; callers build a new index when
/// their workspace snapshot changes. This keeps editor analysis deterministic
/// and lets a language-server session publish one coherent source snapshot to
/// its request handlers. Analyses are cached lazily per path within this
/// snapshot; rebuilding the index invalidates every cached result.
#[derive(Debug)]
pub struct WorkspaceSourceIndex {
    project_root: Utf8PathBuf,
    modules: BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
    settings: SourceAnalysisSettings,

    /// Lazily computed analyses; the map is fixed with the module snapshot.
    analyses: BTreeMap<Utf8PathBuf, OnceLock<SourceAnalysis>>,
    builtin_module: Option<Utf8PathBuf>,
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
        Self::from_shared_modules(project_root, settings, modules.into_iter().map(Arc::new))
    }

    /// Builds an immutable index without copying syntax retained by a project cache.
    ///
    /// Shared modules must have been collected with these settings and project root.
    /// As with `from_modules`, the last module for each path wins.
    pub fn from_shared_modules(
        project_root: Utf8PathBuf,
        settings: SourceAnalysisSettings,
        modules: impl IntoIterator<Item = Arc<CollectedModule>>,
    ) -> Self {
        let modules: BTreeMap<Utf8PathBuf, Arc<CollectedModule>> = modules
            .into_iter()
            .map(|module| (module.path.path().clone(), module))
            .collect();
        let builtin_module = modules
            .values()
            .find(|module| module.path.module_name() == "karva._builtins")
            .map(|module| module.path.path().clone());
        let analyses = modules
            .keys()
            .map(|path| (path.clone(), OnceLock::new()))
            .collect();
        Self {
            project_root,
            modules,
            settings,
            analyses,
            builtin_module,
        }
    }

    /// Collects each source document once and builds an immutable index.
    ///
    /// Returns `None` when any source cannot be collected beneath the project
    /// root. A later document with the same path replaces an earlier one
    /// before collection, so duplicate inputs never cause duplicate parsing
    /// work.
    #[cfg(test)]
    pub(super) fn from_documents(
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

    /// Root used for project-relative CLI selectors and command working directories.
    pub fn project_root(&self) -> &Utf8Path {
        &self.project_root
    }

    /// Returns the collected module for `path`, if the snapshot contains it.
    pub fn module(&self, path: &Utf8Path) -> Option<&CollectedModule> {
        self.modules.get(path).map(AsRef::as_ref)
    }

    /// Returns source paths in deterministic lexical order.
    pub fn paths(&self) -> impl Iterator<Item = &Utf8Path> {
        self.modules
            .keys()
            .filter(|path| self.builtin_module.as_ref() != Some(*path))
            .map(Utf8PathBuf::as_path)
    }

    /// Analyzes one indexed source with visible ancestor `conftest.py` files.
    ///
    /// Ancestors are passed to the source analyzer from the project root
    /// toward the nearest package, matching runtime fixture precedence. The
    /// result is memoized for this immutable snapshot. The returned value is
    /// cloned for its public diagnostics, while its parsed syntax tree and
    /// fixture model are shared by reference-counted storage.
    pub fn analyze(&self, path: &Utf8Path) -> Option<SourceAnalysis> {
        let cached = self.analyses.get(path)?;
        if let Some(analysis) = cached.get() {
            return Some(analysis.clone());
        }

        let current = self.modules.get(path)?.clone();
        let parents = self.parent_modules(path);
        let builtin = self
            .builtin_module
            .as_ref()
            .and_then(|path| self.modules.get(path))
            .map(AsRef::as_ref);
        Some(
            cached
                .get_or_init(|| {
                    analyze_collected_source(current, &parents, builtin, &self.settings)
                })
                .clone(),
        )
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
                parents.push(module.as_ref());
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
    use ruff_text_size::{TextRange, TextSize};
    use std::hint::black_box;
    use std::io::Write;
    use std::sync::Arc;
    use std::time::Instant;

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
            &analysis.fixture_model.local()[0].dependencies[0].resolution,
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
        assert_eq!(analysis.fixture_model.visible().len(), 1);
    }

    #[test]
    fn cache_hits_share_immutable_analysis_but_not_mutable_diagnostics() {
        let path = Utf8Path::new("/project/test_example.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [document(path.as_str(), "def test_example(missing): pass\n")],
            settings(),
        )
        .expect("sources should index");
        let mut first = index.analyze(path).expect("first analysis");
        let second = index.analyze(path).expect("cache hit");
        assert!(Arc::ptr_eq(&first.module, &index.modules[path]));
        assert!(Arc::ptr_eq(&first.module, &second.module));
        assert!(Arc::ptr_eq(&first.fixture_model, &second.fixture_model));
        assert!(!first.diagnostics.is_empty());
        first.diagnostics.clear();
        assert_eq!(
            index.analyze(path).expect("later cache hit").diagnostics,
            second.diagnostics
        );
    }

    #[test]
    fn a_new_snapshot_reanalyzes_unsaved_source() {
        let path = Utf8Path::new("/project/test_example.py");
        let original = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [document(
                path.as_str(),
                "def test_example(database): pass\n",
            )],
            settings(),
        )
        .expect("original source should index");
        assert!(
            original
                .analyze(path)
                .expect("original source should analyze")
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == crate::DiagnosticCode::MissingFixture)
        );

        let edited = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [document(
                path.as_str(),
                "from karva import fixture\n@fixture\ndef database(): pass\n\ndef test_example(database): pass\n",
            )],
            settings(),
        )
        .expect("edited source should index");
        assert!(
            edited
                .analyze(path)
                .expect("edited source should analyze")
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != crate::DiagnosticCode::MissingFixture)
        );
    }

    #[test]
    fn a_new_snapshot_reanalyzes_changed_settings_and_conftest() {
        let path = Utf8Path::new("/project/test_example.py");
        let provider = document(
            "/project/conftest.py",
            "from karva import fixture\n@fixture\ndef database(): pass\n",
        );
        let source = document(path.as_str(), "def spec_example(database): pass\n");

        let original = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [provider.clone(), source.clone()],
            settings(),
        )
        .expect("original source should index");
        let original_analysis = original
            .analyze(path)
            .expect("original source should analyze");
        assert!(original_analysis.diagnostics.is_empty());
        assert!(original_analysis.module.test_function_defs.is_empty());

        let mut changed_settings = settings();
        changed_settings.test_function_prefix = "spec".to_owned();
        let changed_settings_index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [provider, source.clone()],
            changed_settings.clone(),
        )
        .expect("changed settings should index");
        let changed_settings_analysis = changed_settings_index
            .analyze(path)
            .expect("changed source should analyze");
        assert!(changed_settings_analysis.diagnostics.is_empty());
        assert_eq!(changed_settings_analysis.module.test_function_defs.len(), 1);

        let changed_provider = document(
            "/project/conftest.py",
            "from karva import fixture\n@fixture\ndef other(): pass\n",
        );
        let changed_provider_index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [changed_provider, source],
            changed_settings,
        )
        .expect("changed provider should index");
        assert!(
            changed_provider_index
                .analyze(path)
                .expect("changed provider source should analyze")
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == crate::DiagnosticCode::MissingFixture)
        );
    }

    /// Run with `cargo test -p karva_ide benchmark_repeated_fixture_analysis
    /// -- --ignored --nocapture` to compare uncached and cached workloads.
    ///
    /// Receipt: on 2026-10-08, an aarch64 macOS debug build analyzed 66 sources
    /// over 50 passes (3,300 analyses) in 102.09 ms uncached and 11.51 ms with
    /// a warm snapshot cache. These timings measure analysis only, excluding
    /// parsing, cache warmup, filesystem reads and LSP transport.
    #[test]
    #[ignore = "manual benchmark; run before and after cache changes"]
    fn benchmark_repeated_fixture_analysis() {
        let mut documents = vec![
            document(
                "/project/conftest.py",
                "from karva import fixture\n@fixture\ndef database(): pass\n",
            ),
            document(
                "/project/pkg/conftest.py",
                "from karva import fixture\n@fixture\ndef package_fixture(database): pass\n",
            ),
        ];
        for number in 0..64 {
            documents.push(document(
                &format!("/project/pkg/test_{number}.py"),
                "def test_example(database, package_fixture): pass\n",
            ));
        }
        let index = WorkspaceSourceIndex::from_documents("/project".into(), documents, settings())
            .expect("benchmark sources should index");
        let paths = index.paths().collect::<Vec<_>>();
        let uncached_start = Instant::now();
        let mut uncached_analyses = 0;
        for _ in 0..50 {
            for path in &paths {
                let current = index
                    .modules
                    .get(*path)
                    .expect("benchmark path should be indexed")
                    .clone();
                let parents = index.parent_modules(path);
                black_box(analyze_collected_source(
                    current,
                    &parents,
                    None,
                    &index.settings,
                ));
                uncached_analyses += 1;
            }
        }
        let uncached_elapsed = uncached_start.elapsed();

        for path in &paths {
            black_box(index.analyze(path));
        }
        let cached_start = Instant::now();
        let mut cached_analyses = 0;
        for _ in 0..50 {
            for path in &paths {
                cached_analyses += black_box(usize::from(index.analyze(path).is_some()));
            }
        }
        let cached_elapsed = cached_start.elapsed();
        writeln!(
            std::io::stderr(),
            "fixture analysis: uncached={uncached_analyses} in {uncached_elapsed:?}, \
             cached={cached_analyses} in {cached_elapsed:?}, paths={}",
            paths.len(),
        )
        .expect("benchmark report should be writable");
    }
    #[test]
    fn resolves_builtin_definition_from_external_source_module() {
        let collection_settings = CollectionSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test",
            respect_ignore_files: true,
            collect_fixtures: true,
            collect_doctests: false,
        };
        let builtin_path = Utf8Path::new("/venv/lib/python3.12/site-packages/karva/_builtins.py");
        let builtin_source =
            "from karva._karva import fixture\n\n@fixture\ndef tmp_path():\n    yield path\n";
        let builtin = collect_source_with_module_name(
            &builtin_path.to_path_buf(),
            "karva._builtins",
            builtin_source.to_owned(),
            &collection_settings,
            &[],
        )
        .expect("built-in source should collect");
        let test_path = Utf8Path::new("/project/test_example.py").to_path_buf();
        let test_source = "def test_example(tmp_path): pass\n";
        let test = collect_source(
            &test_path,
            Utf8Path::new("/project"),
            test_source.to_owned(),
            &collection_settings,
            &[],
        )
        .expect("test source should collect");
        let index =
            WorkspaceSourceIndex::from_modules("/project".into(), settings(), [test, builtin]);
        assert_eq!(
            index.paths().collect::<Vec<_>>(),
            [Utf8Path::new("/project/test_example.py")]
        );
        assert!(index.module(builtin_path).is_some());
        let analysis = index.analyze(&test_path).expect("test should analyze");
        let offset = TextSize::try_from(test_source.find("tmp_path").expect("parameter marker"))
            .expect("source fits");
        let target = crate::fixture_definition(&analysis, offset).expect("builtin definition");
        assert_eq!(target.path, builtin_path);
        let name_start = TextSize::try_from(builtin_source.find("tmp_path").expect("name marker"))
            .expect("source fits");
        assert_eq!(
            target.range,
            TextRange::new(
                name_start,
                name_start + TextSize::try_from("tmp_path".len()).expect("source fits"),
            )
        );
        let implementation =
            crate::fixture_implementation(&analysis, offset).expect("builtin implementation");
        assert_eq!(implementation.path, builtin_path);
        let implementation_start =
            TextSize::try_from(builtin_source.find("yield path").expect("yield marker"))
                .expect("source fits");
        assert_eq!(
            implementation.range,
            TextRange::new(
                implementation_start,
                implementation_start + TextSize::try_from("yield path".len()).expect("source fits"),
            )
        );
    }

    #[test]
    fn local_fixture_overrides_external_builtin_source() {
        let collection_settings = CollectionSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test",
            respect_ignore_files: true,
            collect_fixtures: true,
            collect_doctests: false,
        };
        let builtin_path = Utf8Path::new("/venv/lib/python3.12/site-packages/karva/_builtins.py");
        let builtin_source = "from karva._karva import fixture\n@fixture\ndef tmp_path(): pass\n";
        let builtin = collect_source_with_module_name(
            &builtin_path.to_path_buf(),
            "karva._builtins",
            builtin_source.to_owned(),
            &collection_settings,
            &[],
        )
        .expect("built-in source should collect");
        let test_path = Utf8Path::new("/project/test_example.py").to_path_buf();
        let test_source = "from karva import fixture\n@fixture\ndef tmp_path(): pass\ndef test_example(tmp_path): pass\n";
        let test = collect_source(
            &test_path,
            Utf8Path::new("/project"),
            test_source.to_owned(),
            &collection_settings,
            &[],
        )
        .expect("test source should collect");
        let index =
            WorkspaceSourceIndex::from_modules("/project".into(), settings(), [test, builtin]);
        let analysis = index.analyze(&test_path).expect("test should analyze");
        let offset = TextSize::try_from(test_source.rfind("tmp_path").expect("parameter marker"))
            .expect("source fits");
        let target = crate::fixture_definition(&analysis, offset).expect("local definition");
        assert_eq!(target.path, test_path);
    }
}
