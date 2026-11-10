//! Fixture import regression tests exercise source identity and runtime exposure names together.

use camino::Utf8Path;
use indoc::indoc;
use karva_collector::{
    CollectedModule, CollectionSettings, collect_source, collect_source_with_module_name,
};
use ruff_python_ast::PythonVersion;
use ruff_text_size::{TextRange, TextSize};

use crate::{
    DiagnosticCode, FixtureResolution, SourceAnalysisSettings, SourceDocument,
    WorkspaceSourceIndex, analyze_sources,
};

fn index(sources: &[(&str, &str)], try_import_fixtures: bool) -> WorkspaceSourceIndex {
    crate::test_support::source_index(
        "/project".into(),
        sources
            .iter()
            .map(|(path, text)| SourceDocument::new((*path).into(), (*text).to_owned())),
        SourceAnalysisSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test".to_owned(),
            try_import_fixtures,
        },
    )
    .expect("sources should collect")
}

fn offset(source: &str, marker: &str) -> TextSize {
    TextSize::try_from(source.find(marker).expect("marker exists")).expect("source fits")
}

#[test]
fn imported_custom_fixture_keeps_its_public_name_and_source_identity() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    @fixture(name='database')
                    def provider(): pass
                "},
            ),
            (
                "/project/conftest.py",
                "from support import provider as alias\n",
            ),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert_eq!(analysis.diagnostics.as_slice(), []);
    let target = crate::fixture_definition(&analysis, offset(test, "database")).expect("fixture");
    assert_eq!(target.path, Utf8Path::new("/project/support.py"));
    let source = &index
        .module(&target.path)
        .expect("provider source")
        .source_text;
    assert_eq!(&source[target.range.to_std_range()], "provider");
    let fixture = crate::fixture_target(&analysis, offset(test, "database")).expect("identity");
    let references = crate::fixture_references(&index, &fixture, true);
    assert!(
        references
            .iter()
            .any(|reference| reference.path == Utf8Path::new("/project/conftest.py"))
    );
}

#[test]
fn follows_relative_reexports_and_keeps_declared_name_through_aliases() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support/provider.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                "},
            ),
            (
                "/project/support/__init__.py",
                "from .provider import database as exported\n",
            ),
            (
                "/project/conftest.py",
                "from support import exported as alias\n",
            ),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert_eq!(analysis.diagnostics.as_slice(), []);
    let target = crate::fixture_definition(&analysis, offset(test, "database")).expect("fixture");
    assert_eq!(target.path, Utf8Path::new("/project/support/provider.py"));
    let fixture = crate::fixture_target(&analysis, offset(test, "database")).expect("fixture");
    assert!(
        crate::fixture_references(&index, &fixture, true)
            .iter()
            .any(|reference| reference.path == Utf8Path::new("/project/support/__init__.py"))
    );
}

#[test]
fn plain_module_import_does_not_hide_missing_fixture_diagnostics() {
    let index = index(
        &[
            ("/project/conftest.py", "import os\n"),
            ("/project/test_query.py", "def test_query(missing): pass\n"),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::MissingFixture)
    );
}

#[test]
fn import_cycles_and_unknown_external_exports_remain_unresolved() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            ("/project/a.py", "from b import database\n"),
            ("/project/b.py", "from a import database\n"),
            (
                "/project/conftest.py",
                indoc! {r"
                    from a import database
                    from external import other
                "},
            ),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert_eq!(analysis.diagnostics.as_slice(), []);
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_none());
}

#[test]
fn overwritten_export_does_not_resolve_to_old_fixture() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                    database = replacement
                "},
            ),
            ("/project/conftest.py", "from support import database\n"),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_none());
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[test]
fn imports_in_tests_require_runtime_discovery_setting() {
    let test = indoc! {r"
        from support import database
        def test_query(database): pass
    "};
    let sources = [
        (
            "/project/support.py",
            indoc! {r"
                from karva import fixture
                @fixture
                def database(): pass
            "},
        ),
        ("/project/test_query.py", test),
    ];
    let enabled = index(&sources, true);
    let analysis = enabled
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database):")).is_some());
    let disabled = index(&sources, false);
    let analysis = disabled
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database):")).is_none());
}

#[test]
fn wildcard_import_uses_public_exports_and_fixture_metadata() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    @fixture(name='database')
                    def provider(): pass
                    @fixture
                    def _private(): pass
                "},
            ),
            ("/project/conftest.py", "from support import *\n"),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    let target = crate::fixture_definition(&analysis, offset(test, "database")).expect("fixture");
    assert_eq!(target.path, Utf8Path::new("/project/support.py"));
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[test]
fn wildcard_import_honors_literal_all_and_empty_all() {
    let private_test = "def test_private(_private): pass\n";
    let empty_test = "def test_empty(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    __all__ = ['_private']
                    @fixture
                    def database(): pass
                    @fixture
                    def _private(): pass
                "},
            ),
            (
                "/project/empty_support.py",
                indoc! {r"
                    from karva import fixture
                    __all__ = []
                    @fixture
                    def database(): pass
                "},
            ),
            ("/project/private/conftest.py", "from support import *\n"),
            (
                "/project/empty/conftest.py",
                "from empty_support import *\n",
            ),
            ("/project/private/test_query.py", private_test),
            ("/project/empty/test_query.py", empty_test),
        ],
        false,
    );
    let private_analysis = index
        .analyze(Utf8Path::new("/project/private/test_query.py"))
        .expect("private analysis");
    assert!(
        crate::fixture_definition(&private_analysis, offset(private_test, "_private):")).is_some()
    );
    let empty_analysis = index
        .analyze(Utf8Path::new("/project/empty/test_query.py"))
        .expect("empty analysis");
    assert!(crate::fixture_definition(&empty_analysis, offset(empty_test, "database")).is_none());
    assert!(
        empty_analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::MissingFixture)
    );
}

#[test]
fn wildcard_import_follows_relative_reexports_and_unknown_all_is_conservative() {
    let test = "def test_query(database): pass\n";
    let reexport_index = index(
        &[
            (
                "/project/support/provider.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                "},
            ),
            ("/project/support/__init__.py", "from .provider import *\n"),
            ("/project/conftest.py", "from support import *\n"),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = reexport_index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_some());

    let unknown_index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    __all__ = make_exports()
                    @fixture
                    def database(): pass
                "},
            ),
            ("/project/conftest.py", "from support import *\n"),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = unknown_index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("unknown analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_none());
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[test]
fn later_rebinding_hides_wildcard_fixture() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                "},
            ),
            (
                "/project/conftest.py",
                indoc! {r"
                    from support import *
                    database = replacement
                "},
            ),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_none());
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[test]
fn known_wildcard_imports_still_report_missing_fixtures() {
    let test = "def test_query(missing): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                "},
            ),
            ("/project/conftest.py", "from support import *\n"),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "missing")).is_none());
    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::MissingFixture)
    );
}

#[rstest::rstest]
fn wildcard_binding_collisions_use_source_name_for_custom_fixture_names(
    #[values(
        indoc! {r"
            from support import *
            provider = replacement
        "},
        "provider = replacement\nfrom support import *\n"
    )]
    conftest: &str,
) {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    @fixture(name='database')
                    def provider(): pass
                "},
            ),
            ("/project/conftest.py", conftest),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_none());
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[test]
fn dynamic_getattr_imports_remain_unknown() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                "def __getattr__(name): return object()\n",
            ),
            ("/project/conftest.py", "from support import database\n"),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_none());
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[test]
fn mutated_literal_all_is_unknown() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    __all__ = ['database']
                    __all__.append('other')
                    @fixture
                    def database(): pass
                "},
            ),
            ("/project/conftest.py", "from support import *\n"),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_none());
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[test]
fn conditional_wildcard_import_is_unknown() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                "},
            ),
            (
                "/project/conftest.py",
                indoc! {r"
                    if condition:
                        from support import *
                "},
            ),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_none());
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[test]
fn conditional_wildcard_reexport_remains_unknown() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/provider.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                "},
            ),
            (
                "/project/support.py",
                indoc! {r"
                    if condition:
                        from provider import *
                "},
            ),
            ("/project/conftest.py", "from support import *\n"),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_none());
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[test]
fn conflicting_wildcard_bindings_remain_unknown() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/first.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                "},
            ),
            (
                "/project/second.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                "},
            ),
            (
                "/project/conftest.py",
                indoc! {r"
                    from first import *
                    from second import *
                "},
            ),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_none());
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[test]
fn wildcard_shadowing_removes_earlier_local_fixture() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    @fixture(name='database')
                    def provider(): pass
                "},
            ),
            (
                "/project/conftest.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                    from support import *
                "},
            ),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_none());
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[test]
fn aliased_all_binding_does_not_hide_wildcard_exports() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                "},
            ),
            (
                "/project/conftest.py",
                indoc! {r"
                    from support import __all__ as exported
                    from support import *
                "},
            ),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_some());
}

#[test]
fn imported_fixture_bound_to_all_remains_navigable() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                "},
            ),
            (
                "/project/conftest.py",
                "from support import database as __all__\n",
            ),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    let definition =
        crate::fixture_definition(&analysis, offset(test, "database")).expect("fixture");
    assert_eq!(definition.path, Utf8Path::new("/project/support.py"));
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[test]
fn all_binding_alias_invalidates_wildcard_exports() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from provider import database as __all__
                    from provider import *
                "},
            ),
            (
                "/project/provider.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                "},
            ),
            ("/project/conftest.py", "from support import *\n"),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_none());
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[test]
fn imported_fixtures_rename_source_bindings_and_reexports() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                indoc! {r"
                    from karva import fixture
                    @fixture
                    def database(): pass
                "},
            ),
            ("/project/conftest.py", "from support import database\n"),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    let target = crate::fixture_rename_target(&analysis, offset(test, "database")).expect("target");
    let edits = crate::rename_fixture(&index, &target.occurrence, "renamed_database")
        .expect("imported fixture should rename");
    assert_eq!(edits.len(), 3);
    assert!(edits.iter().any(|edit| {
        edit.path == Utf8Path::new("/project/support.py")
            && edit.occurrence.kind == crate::FixtureOccurrenceKind::Definition
    }));
    assert!(edits.iter().any(|edit| {
        edit.path == Utf8Path::new("/project/conftest.py")
            && edit.occurrence.kind == crate::FixtureOccurrenceKind::Import
    }));
}

#[rstest::rstest]
fn literal_module_values_do_not_hide_missing_fixtures(
    #[values(
        "30",
        "-30",
        "True",
        "None",
        "...",
        "'hello'",
        "b'hello'",
        "f'hello {30}'",
        "[]",
        "{}",
        "()",
        "{30}",
        "[factory()]"
    )]
    value: &str,
    #[values("setting", "setting: object", "first = second")] target: &str,
) {
    let conftest = format!("{target} = {value}\n");
    let index = index(
        &[
            ("/project/conftest.py", &conftest),
            ("/project/test_query.py", "def test_query(missing): pass\n"),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::MissingFixture)
    );
}

#[rstest::rstest]
fn potentially_callable_module_values_remain_visibility_barriers(
    #[values(
        "setting = factory()",
        "setting = provider",
        "setting = lambda: None",
        "first, second = (provider, 30)",
        "if enabled:\n    setting = 30",
        "setting = [captured := provider]"
    )]
    statement: &str,
) {
    let index = index(
        &[
            ("/project/conftest.py", statement),
            ("/project/test_query.py", "def test_query(missing): pass\n"),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert_eq!(analysis.diagnostics.as_slice(), []);
}

#[rstest::rstest]
fn imported_nonfixture_assignment_does_not_hide_missing_fixtures(
    #[values("30", "[provider]", "'text'")] value: &str,
) {
    let source = format!("setting = {value}\n");
    let index = index(
        &[
            ("/project/support.py", &source),
            ("/project/conftest.py", "from support import setting\n"),
            ("/project/test_query.py", "def test_query(missing): pass\n"),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::MissingFixture)
    );
}

#[test]
fn same_name_overrides_continue_outward_through_all_providers() {
    let root = module(
        "/project/conftest.py",
        "from karva import fixture\n\n@fixture\ndef database(): pass\n",
    );
    let parent = module(
        "/project/pkg/conftest.py",
        "from karva import fixture\n\n@fixture\ndef database(database): pass\n",
    );
    let current = module(
        "/project/pkg/inner/conftest.py",
        "from karva import fixture\n\n@fixture\ndef database(database): pass\n",
    );
    let analysis = analyze_sources(current, &[root, parent], &settings());

    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    let dependency = &analysis.fixture_model().local()[0].dependencies[0].resolution;
    let id = match dependency {
        FixtureResolution::Resolved(id) => Some(id),
        _ => None,
    }
    .expect("expected parent fixture");
    assert_eq!(id.path, "/project/pkg/conftest.py");
    let parent = analysis
        .fixture_model()
        .definition(id)
        .expect("parent definition");
    assert!(matches!(
        &parent.dependencies[0].resolution,
        FixtureResolution::Resolved(id) if id.path == "/project/conftest.py"
    ));
    let offset = TextSize::try_from(
        analysis
            .module()
            .source_text
            .find("database):")
            .expect("parameter"),
    )
    .expect("source fits");
    let target = crate::fixture_definition(&analysis, offset).expect("outer override target");
    assert_eq!(target.path, "/project/pkg/conftest.py");
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
    let builtin_source = indoc! {r"
            from karva._karva import fixture

            @fixture
            def tmp_path():
                yield path
        "};
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
    let index = WorkspaceSourceIndex::from_shared_modules(
        "/project".into(),
        settings(),
        [test, builtin].into_iter().map(std::sync::Arc::new),
    );
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
    let builtin_source = indoc! {r"
            from karva._karva import fixture
            @fixture
            def tmp_path(): pass
        "};
    let builtin = collect_source_with_module_name(
        &builtin_path.to_path_buf(),
        "karva._builtins",
        builtin_source.to_owned(),
        &collection_settings,
        &[],
    )
    .expect("built-in source should collect");
    let test_path = Utf8Path::new("/project/test_example.py").to_path_buf();
    let test_source = indoc! {r"
            from karva import fixture
            @fixture
            def tmp_path(): pass
            def test_example(tmp_path): pass
        "};
    let test = collect_source(
        &test_path,
        Utf8Path::new("/project"),
        test_source.to_owned(),
        &collection_settings,
        &[],
    )
    .expect("test source should collect");
    let index = WorkspaceSourceIndex::from_shared_modules(
        "/project".into(),
        settings(),
        [test, builtin].into_iter().map(std::sync::Arc::new),
    );
    let analysis = index.analyze(&test_path).expect("test should analyze");
    let offset = TextSize::try_from(test_source.rfind("tmp_path").expect("parameter marker"))
        .expect("source fits");
    let target = crate::fixture_definition(&analysis, offset).expect("local definition");
    assert_eq!(target.path, test_path);
}
fn settings() -> SourceAnalysisSettings {
    SourceAnalysisSettings {
        python_version: PythonVersion::PY312,
        test_function_prefix: "test".to_owned(),
        try_import_fixtures: false,
    }
}

fn module(path: &str, source: &str) -> CollectedModule {
    collect_source(
        &path.into(),
        Utf8Path::new("/project"),
        source.to_owned(),
        &CollectionSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test",
            respect_ignore_files: true,
            collect_fixtures: true,
            collect_doctests: false,
        },
        &[],
    )
    .expect("source collects")
}
