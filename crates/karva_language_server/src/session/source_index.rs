//! Deterministic, source-first workspace indexing for editor requests.
//!
//! The prepared input is intentionally owned. A caller can capture project
//! configuration and open-document overlays on the event-loop thread, then
//! build the index on a worker without borrowing [`super::Session`].

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use camino::{Utf8Path, Utf8PathBuf};
use ignore::WalkBuilder;
use ignore::types::Types;
use karva_collector::{CollectionSettings, collect_source};
use karva_ide::{SourceAnalysisSettings, WorkspaceSourceIndex};
use karva_project::path::{TestPath, TestPathError, TestPathFunction, absolute};
use once_cell::sync::OnceCell;
use thiserror::Error;

use super::RequestCancellationToken;

pub(super) type SourceIndexCache = Arc<OnceCell<Arc<WorkspaceSourceIndex>>>;

/// Owned inputs captured before building a workspace source index.
#[derive(Debug)]
pub struct PreparedSourceIndex {
    project_root: Utf8PathBuf,
    include_paths: Vec<String>,
    open_sources: BTreeMap<Utf8PathBuf, String>,
    settings: SourceAnalysisSettings,
    respect_ignore_files: bool,
    cache: SourceIndexCache,
}

impl PreparedSourceIndex {
    /// Creates an index build from project selections and open-document text.
    #[cfg(test)]
    fn new(
        project_root: Utf8PathBuf,
        include_paths: Vec<String>,
        open_sources: BTreeMap<Utf8PathBuf, String>,
        settings: SourceAnalysisSettings,
        respect_ignore_files: bool,
    ) -> Self {
        Self::with_cache(
            project_root,
            include_paths,
            open_sources,
            settings,
            respect_ignore_files,
            Arc::default(),
        )
    }

    pub(super) fn with_cache(
        project_root: Utf8PathBuf,
        include_paths: Vec<String>,
        open_sources: BTreeMap<Utf8PathBuf, String>,
        settings: SourceAnalysisSettings,
        respect_ignore_files: bool,
        cache: SourceIndexCache,
    ) -> Self {
        Self {
            project_root,
            include_paths,
            open_sources,
            settings,
            respect_ignore_files,
            cache,
        }
    }

    /// Builds a deterministic source index while honoring cooperative cancellation.
    pub fn build(
        self,
        cancellation: &RequestCancellationToken,
    ) -> Result<Arc<WorkspaceSourceIndex>, SourceIndexError> {
        let cache = Arc::clone(&self.cache);
        cache
            .get_or_try_init(|| self.build_uncached(cancellation).map(Arc::new))
            .map(Arc::clone)
    }

    fn build_uncached(
        self,
        cancellation: &RequestCancellationToken,
    ) -> Result<WorkspaceSourceIndex, SourceIndexError> {
        let started = Instant::now();
        let Self {
            project_root,
            include_paths,
            open_sources,
            settings,
            respect_ignore_files,
            cache: _,
        } = self;

        let mut paths = BTreeSet::new();
        let test_paths = if include_paths.is_empty() {
            let tests = project_root.join("tests");
            let default_path = if tests.is_dir() {
                tests
            } else {
                project_root.clone()
            };
            vec![TestPath::new(default_path.as_str())]
        } else {
            include_paths
                .iter()
                .map(|path| TestPath::new(absolute(path, &project_root).as_str()))
                .collect()
        };
        for test_path in test_paths {
            check_cancelled(cancellation)?;
            let test_path = test_path.map_err(SourceIndexError::InvalidTestPath)?;
            let root = match &test_path {
                TestPath::Directory(path) | TestPath::File(path) => path,
                TestPath::Function(TestPathFunction { path, .. }) => path,
            };
            if !root.starts_with(&project_root) {
                return Err(SourceIndexError::OutsideProjectRoot {
                    path: root.clone(),
                    project_root,
                });
            }
            reject_symlink_root(root)?;
            match test_path {
                TestPath::Directory(path) => {
                    discover_directory(&path, respect_ignore_files, cancellation, &mut paths)?;
                }
                TestPath::File(path) => {
                    if is_python_path(&path) {
                        paths.insert(path);
                    }
                }
                TestPath::Function(TestPathFunction { path, .. }) => {
                    if is_python_path(&path) {
                        paths.insert(path);
                    }
                }
            }
        }

        for path in open_sources.keys() {
            check_cancelled(cancellation)?;
            if path.starts_with(&project_root) && is_python_path(path) {
                paths.insert(path.clone());
            }
        }

        let initial_paths = paths.clone();
        for path in initial_paths {
            add_ancestor_conftests(
                &project_root,
                &path,
                &open_sources,
                &mut paths,
                respect_ignore_files,
                cancellation,
            )?;
        }

        let collection_settings = CollectionSettings {
            python_version: settings.python_version,
            test_function_prefix: &settings.test_function_prefix,
            respect_ignore_files,
            collect_fixtures: true,
            collect_doctests: false,
        };
        let mut modules = Vec::with_capacity(paths.len());
        let mut read_count = 0_usize;
        let mut source_bytes = 0_usize;
        for path in paths {
            check_cancelled(cancellation)?;
            let source_text = if let Some(source_text) = open_sources.get(&path) {
                source_text.clone()
            } else {
                read_count += 1;
                fs::read_to_string(&path).map_err(|source| SourceIndexError::ReadSource {
                    path: path.clone(),
                    source,
                })?
            };
            source_bytes += source_text.len();
            check_cancelled(cancellation)?;
            let Some(module) =
                collect_source(&path, &project_root, source_text, &collection_settings, &[])
            else {
                return Err(SourceIndexError::CollectSource { path });
            };
            modules.push(module);
        }
        check_cancelled(cancellation)?;

        let index = WorkspaceSourceIndex::from_modules(project_root, settings, modules);
        check_cancelled(cancellation)?;

        tracing::debug!(
            source_count = index.paths().count(),
            source_bytes,
            read_count,
            duration_ms = started.elapsed().as_secs_f64() * 1_000.0,
            "built workspace source index"
        );
        Ok(index)
    }
}

/// Errors that prevent a complete source index from being built.
#[derive(Debug, Error)]
pub enum SourceIndexError {
    /// A configured test path failed to resolve.
    #[error("invalid configured test path: {0}")]
    InvalidTestPath(#[source] TestPathError),

    /// A configured test root lies outside the project module namespace.
    #[error("configured source root `{path}` is outside project root `{project_root}`")]
    OutsideProjectRoot {
        /// Configured source root.
        path: Utf8PathBuf,

        /// Project root used for module names.
        project_root: Utf8PathBuf,
    },

    /// Filesystem walking failed before all roots could be enumerated.
    #[error("failed to walk Python source root `{root}`: {source}")]
    Walk {
        /// Root being enumerated when the error occurred.
        root: Utf8PathBuf,

        /// Underlying walker error.
        #[source]
        source: ignore::Error,
    },

    /// A discovered filesystem path could not be represented as UTF-8.
    #[error("Python source path is not valid UTF-8: {path:?}")]
    NonUtf8Path {
        /// Original filesystem path.
        path: PathBuf,
    },

    /// A discovered source file could not be read.
    #[error("failed to read Python source file `{path}`: {source}")]
    ReadSource {
        /// File that could not be read.
        path: Utf8PathBuf,

        /// Underlying filesystem failure.
        #[source]
        source: std::io::Error,
    },

    /// A discovered source file could not be parsed into a source module.
    #[error("failed to collect Python source file `{path}`")]
    CollectSource {
        /// File whose syntax could not be collected.
        path: Utf8PathBuf,
    },

    /// The Python walker type filter could not be initialized.
    #[error("failed to initialize Python source file filter: {0}")]
    TypeFilter(#[source] ignore::Error),

    /// A source path could not be inspected for symlinks or file type.
    #[error("failed to inspect source path `{path}`: {source}")]
    Metadata {
        /// Path being inspected.
        path: Utf8PathBuf,

        /// Underlying filesystem failure.
        #[source]
        source: std::io::Error,
    },

    /// A configured root is a symlink and is not followed.
    #[error("configured source root is a symlink and will not be followed: `{0}`")]
    SymlinkRoot(Utf8PathBuf),

    /// The request was cancelled while the index was being built.
    #[error("workspace source index build was cancelled")]
    Cancelled,
}

fn discover_directory(
    root: &Utf8Path,
    respect_ignore_files: bool,
    cancellation: &RequestCancellationToken,
    paths: &mut BTreeSet<Utf8PathBuf>,
) -> Result<(), SourceIndexError> {
    let walker = WalkBuilder::new(root)
        .standard_filters(true)
        .require_git(false)
        .git_global(false)
        .parents(true)
        .follow_links(false)
        .git_ignore(respect_ignore_files)
        .types(python_file_types()?)
        .build();

    for entry in walker {
        check_cancelled(cancellation)?;
        let entry = entry.map_err(|source| SourceIndexError::Walk {
            root: root.to_path_buf(),
            source,
        })?;
        if !entry
            .file_type()
            .is_some_and(|file_type| file_type.is_file())
        {
            continue;
        }
        let path = Utf8PathBuf::from_path_buf(entry.into_path())
            .map_err(|path| SourceIndexError::NonUtf8Path { path })?;
        paths.insert(path);
    }
    Ok(())
}

fn reject_symlink_root(path: &Utf8Path) -> Result<(), SourceIndexError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| SourceIndexError::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(SourceIndexError::SymlinkRoot(path.to_path_buf()));
    }
    Ok(())
}

fn python_file_types() -> Result<Types, SourceIndexError> {
    let mut types = ignore::types::TypesBuilder::new();
    types
        .add("python", "*.py")
        .map_err(SourceIndexError::TypeFilter)?;
    types.select("python");
    types.build().map_err(SourceIndexError::TypeFilter)
}

fn add_ancestor_conftests(
    project_root: &Utf8Path,
    path: &Utf8Path,
    open_sources: &BTreeMap<Utf8PathBuf, String>,
    paths: &mut BTreeSet<Utf8PathBuf>,
    respect_ignore_files: bool,
    cancellation: &RequestCancellationToken,
) -> Result<(), SourceIndexError> {
    for directory in ancestor_paths(project_root, path) {
        check_cancelled(cancellation)?;
        let conftest = directory.join("conftest.py");
        if open_sources.contains_key(&conftest)
            || is_discoverable_file(&conftest, respect_ignore_files, cancellation)?
        {
            paths.insert(conftest);
        }
    }
    Ok(())
}

fn is_discoverable_file(
    path: &Utf8Path,
    respect_ignore_files: bool,
    cancellation: &RequestCancellationToken,
) -> Result<bool, SourceIndexError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => return Ok(false),
        Ok(_) => {}
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(SourceIndexError::Metadata {
                path: path.to_path_buf(),
                source,
            });
        }
    }
    let Some(parent) = path.parent() else {
        return Ok(false);
    };
    let walker = WalkBuilder::new(parent)
        .standard_filters(true)
        .require_git(false)
        .git_global(false)
        .parents(true)
        .follow_links(false)
        .git_ignore(respect_ignore_files)
        .types(python_file_types()?)
        .max_depth(Some(1))
        .build();
    for entry in walker {
        check_cancelled(cancellation)?;
        let entry = entry.map_err(|source| SourceIndexError::Walk {
            root: parent.to_path_buf(),
            source,
        })?;
        if entry.path() == path.as_std_path()
            && entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ancestor_paths<'a>(
    project_root: &'a Utf8Path,
    path: &'a Utf8Path,
) -> impl Iterator<Item = Utf8PathBuf> + 'a {
    path.parent()
        .into_iter()
        .flat_map(Utf8Path::ancestors)
        .filter(move |directory| directory.starts_with(project_root))
        .map(Utf8Path::to_path_buf)
}

fn is_python_path(path: &Utf8Path) -> bool {
    path.extension().is_some_and(|extension| extension == "py")
}

fn check_cancelled(cancellation: &RequestCancellationToken) -> Result<(), SourceIndexError> {
    if cancellation.is_cancelled() {
        Err(SourceIndexError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use karva_ide::SourceAnalysisSettings;
    use ruff_python_ast::PythonVersion;

    use super::*;

    struct Fixture {
        _temp_dir: tempfile::TempDir,
        root: Utf8PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temp_dir = tempfile::tempdir().expect("create temp dir");
            let root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf())
                .expect("temporary path should be UTF-8");
            Self {
                _temp_dir: temp_dir,
                root,
            }
        }

        fn write(&self, path: &str, source: &str) -> Utf8PathBuf {
            let path = self.root.join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create source parent");
            }
            fs::write(&path, source).expect("write source");
            path
        }

        fn settings() -> SourceAnalysisSettings {
            SourceAnalysisSettings {
                python_version: PythonVersion::PY312,
                test_function_prefix: "test_".to_owned(),
                try_import_fixtures: false,
            }
        }

        fn build(
            &self,
            include_paths: Vec<String>,
            open_sources: BTreeMap<Utf8PathBuf, String>,
        ) -> Arc<WorkspaceSourceIndex> {
            self.build_with_ignore(include_paths, open_sources, true)
        }

        fn build_with_ignore(
            &self,
            include_paths: Vec<String>,
            open_sources: BTreeMap<Utf8PathBuf, String>,
            respect_ignore_files: bool,
        ) -> Arc<WorkspaceSourceIndex> {
            PreparedSourceIndex::new(
                self.root.clone(),
                include_paths,
                open_sources,
                Self::settings(),
                respect_ignore_files,
            )
            .build(&RequestCancellationToken::default())
            .expect("build source index")
        }
    }

    #[test]
    fn indexes_directory_files_and_ancestor_conftests() {
        let fixture = Fixture::new();
        fixture.write(
            "conftest.py",
            "import pytest\n@pytest.fixture\ndef root(): pass\n",
        );
        fixture.write(
            "tests/conftest.py",
            "import pytest\n@pytest.fixture\ndef nested(): pass\n",
        );
        let test = fixture.write(
            "tests/unit/test_sample.py",
            "def test_example(root, nested): pass\n",
        );
        let index = fixture.build(
            vec![fixture.root.join("tests").to_string()],
            BTreeMap::new(),
        );

        assert!(index.module(&test).is_some());
        assert!(index.module(&fixture.root.join("conftest.py")).is_some());
        assert!(
            index
                .module(&fixture.root.join("tests/conftest.py"))
                .is_some()
        );
        assert!(index.analyze(&test).is_some());
    }

    #[test]
    fn defaults_to_tests_directory_when_present() {
        let fixture = Fixture::new();
        let test = fixture.write("tests/test_sample.py", "def test_example(): pass\n");
        let outside = fixture.write("test_outside.py", "def test_outside(): pass\n");

        let index = fixture.build(Vec::new(), BTreeMap::new());

        assert!(index.module(&test).is_some());
        assert!(index.module(&outside).is_none());
    }

    #[test]
    fn defaults_to_project_root_without_tests_directory() {
        let fixture = Fixture::new();
        let test = fixture.write("test_sample.py", "def test_example(): pass\n");

        let index = fixture.build(Vec::new(), BTreeMap::new());

        assert!(index.module(&test).is_some());
    }

    #[test]
    fn indexes_explicit_file_and_function_roots() {
        let fixture = Fixture::new();
        let file = fixture.write("tests/test_sample.py", "def test_example(): pass\n");
        let index = fixture.build(
            vec![file.to_string(), format!("{file}::test_example")],
            BTreeMap::new(),
        );

        assert_eq!(index.paths().collect::<Vec<_>>(), vec![file.as_path()]);
    }

    #[test]
    fn open_overlay_replaces_disk_and_includes_unsaved_source() {
        let fixture = Fixture::new();
        let disk = fixture.write("tests/test_sample.py", "def test_example(): pass\n");
        let unsaved = fixture.root.join("tests/test_unsaved.py");
        let open_sources = BTreeMap::from([
            (disk.clone(), "def test_changed(): pass\n".to_owned()),
            (unsaved.clone(), "def test_unsaved(): pass\n".to_owned()),
        ]);

        let index = fixture.build(vec![fixture.root.join("tests").to_string()], open_sources);

        assert_eq!(
            index
                .module(&disk)
                .map(|module| module.source_text.as_str()),
            Some("def test_changed(): pass\n")
        );
        assert_eq!(
            index
                .module(&unsaved)
                .map(|module| module.source_text.as_str()),
            Some("def test_unsaved(): pass\n")
        );
    }

    #[test]
    fn unsaved_conftest_supplies_fixture_without_disk_file() {
        let fixture = Fixture::new();
        let test = fixture.write(
            "tests/package/test_sample.py",
            "def test_example(database): pass\n",
        );
        let conftest = fixture.root.join("tests/package/conftest.py");
        let open_sources = BTreeMap::from([(
            conftest.clone(),
            "from karva import fixture\n@fixture\ndef database(): pass\n".to_owned(),
        )]);

        let index = fixture.build(Vec::new(), open_sources);
        let analysis = index.analyze(&test).expect("test source should analyze");
        let database = analysis
            .visible_fixtures
            .iter()
            .find(|definition| definition.name == "database")
            .expect("unsaved fixture should be visible");

        assert_eq!(database.id.path, conftest);
        assert!(analysis.diagnostics.is_empty());
    }

    #[test]
    fn ignores_non_python_and_gitignored_files() {
        let fixture = Fixture::new();
        fixture.write(".gitignore", "ignored.py\n");
        let kept = fixture.write("tests/kept.py", "def test_kept(): pass\n");
        let ignored = fixture.write("tests/ignored.py", "def test_ignored(): pass\n");
        fixture.write("tests/readme.txt", "not Python\n");
        let index = fixture.build(
            vec![fixture.root.join("tests").to_string()],
            BTreeMap::new(),
        );

        assert!(index.module(&kept).is_some());
        assert!(index.module(&ignored).is_none());
        assert_eq!(index.paths().count(), 1);
    }

    #[test]
    fn can_include_gitignored_files_when_configured() {
        let fixture = Fixture::new();
        fixture.write(".gitignore", "ignored.py\n");
        let ignored = fixture.write("tests/ignored.py", "def test_ignored(): pass\n");

        let index = fixture.build_with_ignore(
            vec![fixture.root.join("tests").to_string()],
            BTreeMap::new(),
            false,
        );

        assert!(index.module(&ignored).is_some());
    }

    #[test]
    fn ignored_ancestor_conftest_is_not_reintroduced() {
        let fixture = Fixture::new();
        fixture.write(".gitignore", "conftest.py\n");
        let conftest = fixture.write(
            "conftest.py",
            "from karva import fixture\n@fixture\ndef database(): pass\n",
        );
        fixture.write("tests/test_sample.py", "def test_example(database): pass\n");

        let index = fixture.build(Vec::new(), BTreeMap::new());

        assert!(index.module(&conftest).is_none());
    }

    #[test]
    fn can_include_ignored_ancestor_conftest_when_configured() {
        let fixture = Fixture::new();
        fixture.write(".gitignore", "conftest.py\n");
        let conftest = fixture.write(
            "conftest.py",
            "from karva import fixture\n@fixture\ndef database(): pass\n",
        );
        fixture.write("tests/test_sample.py", "def test_example(database): pass\n");

        let index = fixture.build_with_ignore(Vec::new(), BTreeMap::new(), false);

        assert!(index.module(&conftest).is_some());
    }

    #[test]
    fn resolves_relative_include_from_project_root() {
        let fixture = Fixture::new();
        let test = fixture.write("specs/test_sample.py", "def test_example(): pass\n");

        let index = fixture.build(vec!["specs".to_owned()], BTreeMap::new());

        assert!(index.module(&test).is_some());
    }

    #[test]
    fn overlapping_roots_are_deduplicated_in_path_order() {
        let fixture = Fixture::new();
        let first = fixture.write("tests/a_test.py", "def test_a(): pass\n");
        let second = fixture.write("tests/nested/z_test.py", "def test_z(): pass\n");

        let index = fixture.build(
            vec!["tests".to_owned(), "tests/nested".to_owned()],
            BTreeMap::new(),
        );

        assert_eq!(
            index.paths().collect::<Vec<_>>(),
            [first.as_path(), second.as_path()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_directory_symlinks() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        fixture.write("tests/test_sample.py", "def test_example(): pass\n");
        let linked = fixture.write("other/test_linked.py", "def test_linked(): pass\n");
        symlink(
            fixture.root.join("other"),
            fixture.root.join("tests/linked"),
        )
        .expect("create directory symlink");
        let index = fixture.build(
            vec![fixture.root.join("tests").to_string()],
            BTreeMap::new(),
        );

        assert!(index.module(&linked).is_none());
    }

    #[test]
    fn reports_invalid_test_paths() {
        let fixture = Fixture::new();
        let error = fixture
            .build_result(vec![fixture.root.join("missing").to_string()])
            .expect_err("invalid test path should fail");
        assert!(matches!(error, SourceIndexError::InvalidTestPath(_)));
    }

    #[test]
    fn preserves_built_snapshot_after_disk_changes() {
        let fixture = Fixture::new();
        let path = fixture.write("tests/test_sample.py", "def test_before(): pass\n");
        let index = fixture.build(Vec::new(), BTreeMap::new());

        fs::write(&path, "def test_after(): pass\n").expect("replace disk source");

        assert_eq!(
            index
                .module(&path)
                .map(|module| module.source_text.as_str()),
            Some("def test_before(): pass\n")
        );
    }

    #[test]
    fn reuses_cached_snapshot_until_session_invalidation() {
        let fixture = Fixture::new();
        let path = fixture.write("tests/test_sample.py", "def test_before(): pass\n");
        let cache = SourceIndexCache::default();
        let prepare = |cache| {
            PreparedSourceIndex::with_cache(
                fixture.root.clone(),
                Vec::new(),
                BTreeMap::new(),
                Fixture::settings(),
                true,
                cache,
            )
        };
        let first = prepare(Arc::clone(&cache))
            .build(&RequestCancellationToken::default())
            .expect("build first snapshot");
        fs::write(&path, "def test_after(): pass\n").expect("replace disk source");

        let cached = prepare(cache)
            .build(&RequestCancellationToken::default())
            .expect("reuse source snapshot");
        let rebuilt = prepare(SourceIndexCache::default())
            .build(&RequestCancellationToken::default())
            .expect("build invalidated snapshot");

        assert!(Arc::ptr_eq(&first, &cached));
        assert_eq!(
            rebuilt
                .module(&path)
                .map(|module| module.source_text.as_str()),
            Some("def test_after(): pass\n")
        );
    }

    #[test]
    fn stops_before_building_when_cancelled() {
        let fixture = Fixture::new();
        fixture.write("tests/test_sample.py", "def test_example(): pass\n");
        let cancellation = RequestCancellationToken::default();
        cancellation.cancel();

        let error = PreparedSourceIndex::new(
            fixture.root,
            Vec::new(),
            BTreeMap::new(),
            Fixture::settings(),
            true,
        )
        .build(&cancellation)
        .expect_err("cancelled build should stop");

        assert!(matches!(error, SourceIndexError::Cancelled));
    }

    #[test]
    fn rejects_configured_root_outside_project() {
        let fixture = Fixture::new();
        let outside = tempfile::tempdir().expect("create outside directory");
        let outside = Utf8PathBuf::from_path_buf(outside.path().to_path_buf())
            .expect("outside path should be UTF-8");
        fs::write(outside.join("test_sample.py"), "def test_example(): pass\n")
            .expect("write outside source");

        let error = fixture
            .build_result(vec![outside.to_string()])
            .expect_err("outside root should fail");

        assert!(matches!(error, SourceIndexError::OutsideProjectRoot { .. }));
    }

    impl Fixture {
        fn build_result(
            &self,
            include_paths: Vec<String>,
        ) -> Result<Arc<WorkspaceSourceIndex>, SourceIndexError> {
            PreparedSourceIndex::new(
                self.root.clone(),
                include_paths,
                BTreeMap::new(),
                Self::settings(),
                true,
            )
            .build(&RequestCancellationToken::default())
        }
    }
}
