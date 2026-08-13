use ruff_text_size::TextRange;

use crate::{FixtureOccurrence, WorkspaceSourceIndex, fixture_references};

/// Returns the current occurrence range when a fixture can be renamed safely.
///
/// Every occurrence resolving to the same provider must have one complete edit
/// range. Implicitly concatenated string literals therefore disable the whole
/// rename instead of producing a partial workspace edit.
pub fn prepare_fixture_rename(
    index: &WorkspaceSourceIndex,
    current: &FixtureOccurrence,
) -> Option<TextRange> {
    let current_range = current.edit_range?;
    let occurrences = fixture_references(index, &current.fixture, true);
    (!occurrences.is_empty()
        && occurrences
            .iter()
            .all(|occurrence| occurrence.occurrence.edit_range.is_some()))
    .then_some(current_range)
}

#[cfg(test)]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use ruff_python_ast::PythonVersion;
    use ruff_text_size::TextSize;

    use super::*;
    use crate::{SourceAnalysisSettings, SourceDocument, fixture_occurrence};

    fn settings() -> SourceAnalysisSettings {
        SourceAnalysisSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test_".to_owned(),
            try_import_fixtures: false,
        }
    }

    fn offset(source: &str, marker: &str) -> TextSize {
        TextSize::try_from(source.find(marker).expect("marker should exist"))
            .expect("source should fit")
    }

    #[test]
    fn prepares_custom_fixture_name() {
        let source = "import pytest\nfrom karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\n@pytest.mark.usefixtures(\"database\")\ndef test_example(): pass\n";
        let path = Utf8PathBuf::from("/project/test_example.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [SourceDocument::new(path.clone(), source.to_owned())],
            settings(),
        )
        .expect("source should index");
        let analysis = index.analyze(&path).expect("source should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(source, "database\")\ndef test"))
            .expect("metadata should resolve");

        let range = prepare_fixture_rename(&index, &occurrence).expect("fixture should rename");

        assert_eq!(&source[range.to_std_range()], "database");
    }

    #[test]
    fn rejects_rename_when_any_occurrence_is_not_editable() {
        let provider =
            "from karva import fixture\n@fixture(name='data' 'base')\ndef provider(): pass\n";
        let test = "def test_example(database): pass\n";
        let path = Utf8Path::new("/project/test_example.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [
                SourceDocument::new("/project/conftest.py".into(), provider.to_owned()),
                SourceDocument::new(path.to_path_buf(), test.to_owned()),
            ],
            settings(),
        )
        .expect("sources should index");
        let analysis = index.analyze(path).expect("test should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(test, "database"))
            .expect("parameter should resolve");

        assert!(prepare_fixture_rename(&index, &occurrence).is_none());
    }
}
