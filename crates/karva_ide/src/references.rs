use camino::Utf8PathBuf;

use crate::{
    FixtureId, FixtureOccurrence, FixtureOccurrenceKind, WorkspaceSourceIndex, fixture_occurrences,
};

/// Fixture occurrence paired with its source path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedFixtureOccurrence {
    /// File containing the occurrence.
    pub path: Utf8PathBuf,

    /// Resolved fixture occurrence within the file.
    pub occurrence: FixtureOccurrence,
}

/// Finds every indexed occurrence resolving to `target`.
///
/// Results follow deterministic source-path and source-range order. Callers
/// resolve the target first, so built-in, missing, rejected, and dynamic
/// fixture references never reach this query.
pub fn fixture_references(
    index: &WorkspaceSourceIndex,
    target: &FixtureId,
    include_declaration: bool,
) -> Vec<LocatedFixtureOccurrence> {
    let mut references = Vec::new();

    for path in index.paths() {
        let Some(analysis) = index.analyze(path) else {
            continue;
        };
        references.extend(
            fixture_occurrences(&analysis)
                .into_iter()
                .filter(|occurrence| &occurrence.fixture == target)
                .filter(|occurrence| {
                    include_declaration || occurrence.kind != FixtureOccurrenceKind::Definition
                })
                .map(|occurrence| LocatedFixtureOccurrence {
                    path: path.to_path_buf(),
                    occurrence,
                }),
        );
    }

    references
}

#[cfg(test)]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use ruff_python_ast::PythonVersion;
    use ruff_text_size::TextSize;

    use super::*;
    use crate::{SourceAnalysisSettings, SourceDocument};

    fn settings() -> SourceAnalysisSettings {
        SourceAnalysisSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test_".to_owned(),
            try_import_fixtures: false,
        }
    }

    fn document(path: &str, source: &str) -> SourceDocument {
        SourceDocument::new(path.into(), source.to_owned())
    }

    fn offset(source: &str, marker: &str) -> TextSize {
        TextSize::try_from(source.find(marker).expect("marker should exist"))
            .expect("source should fit")
    }

    #[test]
    fn finds_cross_file_references_with_optional_declaration() {
        let provider = "from karva import fixture\n@fixture\ndef database(): pass\n";
        let parameter = "def test_parameter(database): pass\n";
        let metadata =
            "import pytest\n@pytest.mark.usefixtures(\"database\")\ndef test_metadata(): pass\n";
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [
                document("/project/conftest.py", provider),
                document("/project/tests/a_test.py", parameter),
                document("/project/tests/b_test.py", metadata),
            ],
            settings(),
        )
        .expect("sources should index");

        let analysis = index
            .analyze(Utf8Path::new("/project/tests/a_test.py"))
            .expect("test should analyze");
        let target =
            crate::occurrences::fixture_occurrence(&analysis, offset(parameter, "database"))
                .expect("fixture should resolve")
                .fixture;
        let references = fixture_references(&index, &target, true);
        assert_eq!(
            references
                .iter()
                .map(|reference| (reference.path.as_str(), reference.occurrence.kind))
                .collect::<Vec<_>>(),
            [
                ("/project/conftest.py", FixtureOccurrenceKind::Definition),
                (
                    "/project/tests/a_test.py",
                    FixtureOccurrenceKind::TestParameter,
                ),
                (
                    "/project/tests/b_test.py",
                    FixtureOccurrenceKind::UseFixtures,
                ),
            ]
        );

        let references = fixture_references(&index, &target, false);
        assert!(
            references.iter().all(|reference| {
                reference.occurrence.kind != FixtureOccurrenceKind::Definition
            })
        );
    }

    #[test]
    fn keeps_shadowed_provider_references_separate() {
        let root_provider = "from karva import fixture\n@fixture\ndef database(): pass\n";
        let nested_provider = "from karva import fixture\n@fixture\ndef database(database): pass\n";
        let root_test = "def test_root(database): pass\n";
        let nested_test = "def test_nested(database): pass\n";
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [
                document("/project/conftest.py", root_provider),
                document("/project/pkg/conftest.py", nested_provider),
                document("/project/test_root.py", root_test),
                document("/project/pkg/test_nested.py", nested_test),
            ],
            settings(),
        )
        .expect("sources should index");

        let nested_analysis = index
            .analyze(Utf8Path::new("/project/pkg/test_nested.py"))
            .expect("nested test should analyze");
        let nested_target = crate::occurrences::fixture_occurrence(
            &nested_analysis,
            offset(nested_test, "database"),
        )
        .expect("nested fixture should resolve")
        .fixture;
        let nested = fixture_references(&index, &nested_target, true);
        assert_eq!(
            nested
                .iter()
                .map(|reference| reference.path.as_str())
                .collect::<Vec<_>>(),
            ["/project/pkg/conftest.py", "/project/pkg/test_nested.py"]
        );

        let provider_analysis = index
            .analyze(Utf8Path::new("/project/pkg/conftest.py"))
            .expect("nested provider should analyze");
        let root_target = crate::occurrences::fixture_occurrence(
            &provider_analysis,
            offset(nested_provider, "database):"),
        )
        .expect("outer fixture dependency should resolve")
        .fixture;
        let root = fixture_references(&index, &root_target, true);
        assert_eq!(
            root.iter()
                .map(|reference| reference.path.as_str())
                .collect::<Vec<_>>(),
            [
                "/project/conftest.py",
                "/project/pkg/conftest.py",
                "/project/test_root.py"
            ]
        );
    }

    #[test]
    fn reports_custom_public_name_ranges() {
        let source = "import pytest\nfrom karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\n@pytest.mark.usefixtures(\"database\")\ndef test_example(): pass\n";
        let path = Utf8PathBuf::from("/project/test_example.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [SourceDocument::new(path.clone(), source.to_owned())],
            settings(),
        )
        .expect("source should index");

        let analysis = index.analyze(&path).expect("source should analyze");
        let target = crate::occurrences::fixture_occurrence(
            &analysis,
            offset(source, "database\")\ndef test"),
        )
        .expect("custom fixture should resolve")
        .fixture;
        let references = fixture_references(&index, &target, true);

        assert_eq!(references.len(), 2);
        assert!(
            references.iter().all(|reference| {
                &source[reference.occurrence.range.to_std_range()] == "database"
            })
        );
    }
}
