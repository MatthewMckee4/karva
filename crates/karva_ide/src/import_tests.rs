//! Fixture import regression tests exercise source identity and runtime exposure names together.

use camino::Utf8Path;
use ruff_python_ast::PythonVersion;
use ruff_text_size::TextSize;

use crate::{DiagnosticCode, SourceAnalysisSettings, SourceDocument, WorkspaceSourceIndex};

fn index(sources: &[(&str, &str)], try_import_fixtures: bool) -> WorkspaceSourceIndex {
    WorkspaceSourceIndex::from_documents(
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
                "from karva import fixture\n@fixture(name='database')\ndef provider(): pass\n",
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
    assert!(analysis.diagnostics.is_empty());
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
                "from karva import fixture\n@fixture\ndef database(): pass\n",
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
    assert!(analysis.diagnostics.is_empty());
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
                "from a import database\nfrom external import other\n",
            ),
            ("/project/test_query.py", test),
        ],
        false,
    );
    let analysis = index
        .analyze(Utf8Path::new("/project/test_query.py"))
        .expect("analysis");
    assert!(analysis.diagnostics.is_empty());
    assert!(crate::fixture_definition(&analysis, offset(test, "database")).is_none());
}

#[test]
fn overwritten_export_does_not_resolve_to_old_fixture() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                "from karva import fixture\n@fixture\ndef database(): pass\ndatabase = replacement\n",
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
    assert!(analysis.diagnostics.is_empty());
}

#[test]
fn imports_in_tests_require_runtime_discovery_setting() {
    let test = "from support import database\ndef test_query(database): pass\n";
    let sources = [
        (
            "/project/support.py",
            "from karva import fixture\n@fixture\ndef database(): pass\n",
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
fn imported_fixtures_rename_source_bindings_and_reexports() {
    let test = "def test_query(database): pass\n";
    let index = index(
        &[
            (
                "/project/support.py",
                "from karva import fixture\n@fixture\ndef database(): pass\n",
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
    assert!(analysis.diagnostics.is_empty());
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
