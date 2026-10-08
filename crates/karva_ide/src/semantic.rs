//! Python binding and type queries backed by ty's incremental semantic database.
//!
//! The database sees exactly the immutable source snapshot supplied by Karva, including unsaved
//! documents. Its filesystem cannot execute Python or read unrelated project files.

#![expect(
    clippy::redundant_pub_crate,
    reason = "Sibling IDE modules consume the private ty adapter"
)]

use std::collections::{BTreeMap, HashMap};
use std::ops::Deref;
use std::sync::{Arc, Mutex, OnceLock};

use camino::{Utf8Path, Utf8PathBuf};
use karva_collector::CollectedModule;
pub(super) use karva_python_semantic::source_analysis::Binding;
use karva_python_semantic::source_analysis::{PythonSourceCache, PythonSourceSnapshot};
use ruff_python_ast::{PythonVersion, Stmt, StmtFunctionDef};
use ruff_text_size::Ranged;

/// Immutable source view with snapshot-local results and shared incremental ty queries.
#[derive(Debug)]
pub(super) struct PythonSemantics {
    cache: Arc<PythonSemanticCache>,
    root: Utf8PathBuf,
    modules: Arc<BTreeMap<Utf8PathBuf, Arc<CollectedModule>>>,
    source: PythonSourceSnapshot<CollectedModule>,
    raw_providers: BTreeMap<Utf8PathBuf, OnceLock<crate::fixture::FixtureProvider>>,
    providers: BTreeMap<Utf8PathBuf, OnceLock<crate::fixture::FixtureProvider>>,
    value_types: HashMap<crate::FixtureId, OnceLock<Option<String>>>,
}

/// Incremental ty queries retained across immutable editor source generations.
///
/// Database access is serialized so a source update never cancels another request's query.
/// Each query activates its own immutable source view, including requests for older snapshots.
#[derive(Debug, Default)]
pub struct PythonSemanticCache {
    source: Arc<PythonSourceCache<CollectedModule>>,

    /// Framework metadata depends on collected syntax, independently of inferred Python types.
    providers:
        Mutex<BTreeMap<Utf8PathBuf, (Arc<CollectedModule>, crate::fixture::FixtureProvider)>>,
}

impl Deref for PythonSemantics {
    type Target = PythonSourceSnapshot<CollectedModule>;

    fn deref(&self) -> &Self::Target {
        &self.source
    }
}

impl PythonSemantics {
    /// Creates isolated semantics without reading project files or executing Python.
    #[cfg(test)]
    fn new(
        root: &Utf8Path,
        modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
        python_version: PythonVersion,
    ) -> Self {
        Self::with_cache(root, modules, python_version, Arc::default())
    }

    pub(super) fn with_cache(
        root: &Utf8Path,
        modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
        python_version: PythonVersion,
        cache: Arc<PythonSemanticCache>,
    ) -> Self {
        let modules = Arc::new(modules.clone());
        Self {
            source: PythonSourceSnapshot::new(
                root.to_owned(),
                Arc::clone(&modules),
                python_version,
                Arc::clone(&cache.source),
            ),
            cache,
            root: root.to_owned(),
            modules: Arc::clone(&modules),
            raw_providers: modules
                .keys()
                .map(|path| (path.clone(), OnceLock::new()))
                .collect(),
            providers: modules
                .keys()
                .map(|path| (path.clone(), OnceLock::new()))
                .collect(),
            value_types: modules
                .values()
                .flat_map(|module| {
                    module
                        .module_body
                        .iter()
                        .filter_map(|statement| {
                            if let Stmt::FunctionDef(function) = statement {
                                Some(function)
                            } else {
                                None
                            }
                        })
                        .map(|function| {
                            (
                                crate::FixtureId {
                                    path: module.path.path().clone(),
                                    range: function.name.range(),
                                },
                                OnceLock::new(),
                            )
                        })
                })
                .collect(),
        }
    }

    /// Shares framework metadata and inferred value types between importing modules.
    pub(super) fn raw_provider(&self, module: &CollectedModule) -> crate::fixture::FixtureProvider {
        let build = || {
            let Some(source) = self.modules.get(module.path.path()) else {
                return crate::fixture::parse_provider(module, self);
            };
            let mut providers = self
                .cache
                .providers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((previous, provider)) = providers.get(module.path.path())
                && Arc::ptr_eq(previous, source)
            {
                return provider.clone();
            }
            let provider = crate::fixture::parse_provider(module, self);
            providers.insert(
                module.path.path().to_owned(),
                (Arc::clone(source), provider.clone()),
            );
            provider
        };
        self.raw_providers
            .get(module.path.path())
            .map_or_else(build, |cached| cached.get_or_init(build).clone())
    }

    /// Infers only a requested provider value, avoiding type-checking during fixture diagnostics.
    pub(super) fn value_type(&self, id: &crate::FixtureId) -> Option<String> {
        self.value_types
            .get(id)?
            .get_or_init(|| {
                let module = self.modules.get(&id.path)?;
                let function = module
                    .module_body
                    .iter()
                    .find_map(|statement| match statement {
                        Stmt::FunctionDef(function) if function.name.range() == id.range => {
                            Some(function)
                        }
                        _ => None,
                    })?;
                self.fixture_value_type(&id.path, function)
            })
            .clone()
    }

    /// Parses each provider once per immutable snapshot, independently of its consumers.
    pub(super) fn provider(
        &self,
        module: &CollectedModule,
        modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
        try_import_fixtures: bool,
    ) -> crate::fixture::FixtureProvider {
        let build = || {
            crate::fixture::imports::provider_with_imports(
                module,
                self.root.as_path(),
                modules,
                self,
                try_import_fixtures,
            )
        };
        self.providers
            .get(module.path.path())
            .map_or_else(build, |cached| cached.get_or_init(build).clone())
    }

    /// Fixture injection exposes yielded values rather than generator objects.
    fn fixture_value_type(&self, path: &Utf8Path, function: &StmtFunctionDef) -> Option<String> {
        self.function_value_type(
            path,
            function,
            crate::fixture::fixture_implementation_range(function) != function.name.range(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karva_collector::{CollectionSettings, collect_source};
    use ruff_python_ast::PythonVersion;

    #[test]
    fn incremental_cache_preserves_old_source_views() {
        let root = Utf8Path::new("/project");
        let path = root.join("test_value.py");
        let settings = CollectionSettings {
            python_version: PythonVersion::PY314,
            test_function_prefix: "test",
            respect_ignore_files: true,
            collect_fixtures: true,
            collect_doctests: false,
        };
        let collect = |value: &str| {
            Arc::new(
                collect_source(
                    &path,
                    root,
                    format!(
                        "from karva import fixture\n@fixture\ndef sample():\n    return {value}\n"
                    ),
                    &settings,
                    &[],
                )
                .expect("source"),
            )
        };
        let first = collect("len('hello')");
        let second = collect("True");
        let cache = Arc::default();
        let old = PythonSemantics::with_cache(
            root,
            &BTreeMap::from([(path.clone(), first.clone())]),
            settings.python_version,
            Arc::clone(&cache),
        );
        let new = PythonSemantics::with_cache(
            root,
            &BTreeMap::from([(path.clone(), second.clone())]),
            settings.python_version,
            cache,
        );
        assert_eq!(
            old.fixture_value_type(&path, &first.fixture_function_defs[0])
                .as_deref(),
            Some("int")
        );
        assert_eq!(
            new.fixture_value_type(&path, &second.fixture_function_defs[0])
                .as_deref(),
            Some("bool")
        );
        assert_eq!(
            old.fixture_value_type(&path, &first.fixture_function_defs[0])
                .as_deref(),
            Some("int")
        );
    }

    #[rstest::rstest]
    #[case("return True", Some("bool"))]
    #[case("value = 1 + 2\n    return value", Some("int"))]
    #[case("return len('hello')", Some("int"))]
    #[case("return missing()", None)]
    #[case("yield 'hello'", Some("str"))]
    #[case("yield 'hello'\n    return 1", Some("str"))]
    #[case("if missing():\n        return 1\n    return 'hello'", None)]
    fn infers_fixture_expressions(#[case] body: &str, #[case] expected: Option<&str>) {
        let source = format!("from karva import fixture\n@fixture\ndef value():\n    {body}\n");
        let root = Utf8Path::new("/project");
        let path = root.join("test_value.py");
        let settings = CollectionSettings {
            python_version: PythonVersion::PY314,
            test_function_prefix: "test",
            respect_ignore_files: true,
            collect_fixtures: true,
            collect_doctests: false,
        };
        let module = Arc::new(collect_source(&path, root, source, &settings, &[]).expect("source"));
        let modules = BTreeMap::from([(path.clone(), Arc::clone(&module))]);
        let semantics = PythonSemantics::new(root, &modules, settings.python_version);
        assert_eq!(
            semantics
                .fixture_value_type(&path, &module.fixture_function_defs[0])
                .as_deref(),
            expected
        );
    }

    #[rstest::rstest]
    #[case("int", "return 1", Some("int"))]
    #[case("'int'", "return 1", Some("int"))]
    #[case("'Iterator[int]'", "yield 1", Some("int"))]
    #[case("list[str]", "return []", Some("list[str]"))]
    #[case("Iterator[int]", "yield 1", Some("int"))]
    #[case("Missing", "return 1", None)]
    #[case("int", "pass", Some("int"))]
    #[case("Iterator[int, str]", "yield 1", None)]
    fn resolves_annotations(
        #[case] annotation: &str,
        #[case] body: &str,
        #[case] expected: Option<&str>,
    ) {
        let source = format!(
            "from typing import Iterator\nfrom karva import fixture\n@fixture\ndef value() -> {annotation}:\n    {body}\n"
        );
        let root = Utf8Path::new("/project");
        let path = root.join("test_value.py");
        let settings = CollectionSettings {
            python_version: PythonVersion::PY314,
            test_function_prefix: "test",
            respect_ignore_files: true,
            collect_fixtures: true,
            collect_doctests: false,
        };
        let module = Arc::new(collect_source(&path, root, source, &settings, &[]).expect("source"));
        let modules = BTreeMap::from([(path.clone(), Arc::clone(&module))]);
        let semantics = PythonSemantics::new(root, &modules, settings.python_version);
        assert_eq!(
            semantics
                .fixture_value_type(&path, &module.fixture_function_defs[0])
                .as_deref(),
            expected
        );
    }
}
