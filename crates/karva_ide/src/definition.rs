use camino::Utf8PathBuf;
use ruff_text_size::{TextRange, TextSize};

use crate::{SourceAnalysis, hover_fixture};

/// A source location for a fixture definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureDefinitionTarget {
    /// File containing the fixture provider.
    pub path: Utf8PathBuf,

    /// UTF-8 byte range of the provider function name.
    pub range: TextRange,
}

/// Resolves a fixture reference to its provider function name.
///
/// This follows the same conservative resolution as [`hover_fixture`].
/// Built-ins, rejected definitions, missing providers, and dynamic providers
/// do not produce a target.
pub fn fixture_definition(
    analysis: &SourceAnalysis,
    offset: TextSize,
) -> Option<FixtureDefinitionTarget> {
    let hover = hover_fixture(analysis, offset)?;
    let provider = hover.provider?;
    let definition = analysis
        .visible_fixtures
        .iter()
        .find(|definition| definition.id == provider)?;
    Some(FixtureDefinitionTarget {
        path: definition.id.path.clone(),
        range: definition.name_range,
    })
}

#[cfg(test)]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use ruff_python_ast::PythonVersion;
    use ruff_text_size::{TextRange, TextSize};

    use super::*;
    use crate::{
        SourceAnalysisSettings, SourceDocument, analyze_source, analyze_source_with_parents,
    };

    fn settings() -> SourceAnalysisSettings {
        SourceAnalysisSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test".to_owned(),
            try_import_fixtures: false,
        }
    }

    fn analysis(source: &str) -> crate::SourceAnalysis {
        analyze_source(
            &Utf8PathBuf::from("/project/test_example.py"),
            Utf8Path::new("/project"),
            source.to_owned(),
            &settings(),
        )
        .expect("source should analyze")
    }

    fn offset(source: &str, marker: &str) -> TextSize {
        TextSize::try_from(source.find(marker).expect("marker exists")).expect("source fits")
    }

    fn offset_last(source: &str, marker: &str) -> TextSize {
        TextSize::try_from(source.rfind(marker).expect("marker exists")).expect("source fits")
    }

    fn expected(source: &str, marker: &str) -> TextRange {
        let start = offset(source, marker);
        TextRange::new(
            start,
            start + TextSize::try_from(marker.len()).expect("marker fits"),
        )
    }

    #[test]
    fn resolves_test_parameter_fixture_to_function_name() {
        let source = "from karva import fixture\n@fixture\ndef database(): pass\ndef test_example(database): pass\n";
        let target = fixture_definition(&analysis(source), offset(source, "database):"))
            .expect("definition target");
        assert_eq!(target.path, "/project/test_example.py");
        assert_eq!(target.range, expected(source, "database"));
    }

    #[test]
    fn resolves_fixture_dependency_to_parent_provider() {
        let parent_source = "from karva import fixture\n@fixture\ndef database(): pass\n";
        let source = "from karva import fixture\n@fixture\ndef wrapper(database): pass\ndef test_example(wrapper): pass\n";
        let parent = SourceDocument::new(
            Utf8PathBuf::from("/project/conftest.py"),
            parent_source.to_owned(),
        );
        let current = SourceDocument::new(
            Utf8PathBuf::from("/project/test_example.py"),
            source.to_owned(),
        );
        let analysis =
            analyze_source_with_parents(current, [parent], Utf8Path::new("/project"), &settings())
                .expect("source should analyze");
        let target =
            fixture_definition(&analysis, offset(source, "database):")).expect("definition target");
        assert_eq!(target.path, "/project/conftest.py");
        assert_eq!(target.range, expected(parent_source, "database"));
    }

    #[test]
    fn resolves_use_fixtures_and_custom_name() {
        let source = "import pytest\nfrom karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\n@pytest.mark.usefixtures(\"database\")\ndef test_example(): pass\n";
        let target = fixture_definition(&analysis(source), offset_last(source, "database\""))
            .expect("definition target");
        assert_eq!(target.path, "/project/test_example.py");
        assert_eq!(target.range, expected(source, "provider"));
    }

    #[test]
    fn resolves_karva_use_fixtures_and_unicode() {
        let source = "import karva\nfrom karva import fixture\n@fixture\ndef δεδομενα(): pass\n@karva.tags.use_fixtures(\"δεδομενα\")\ndef test_example(): pass\n";
        let target = fixture_definition(
            &analysis(source),
            offset(source, "δεδομενα\"")
                + TextSize::try_from("δεδομενα".len()).expect("source fits"),
        )
        .expect("definition target");
        assert_eq!(target.path, "/project/test_example.py");
        assert_eq!(target.range, expected(source, "δεδομενα"));
    }

    #[test]
    fn suppresses_builtin_unknown_and_rejected_targets() {
        let builtin = "def test_example(tmp_path): pass\n";
        assert!(fixture_definition(&analysis(builtin), offset(builtin, "tmp_path")).is_none());

        let unknown = "def test_example(missing): pass\n";
        assert!(fixture_definition(&analysis(unknown), offset(unknown, "missing")).is_none());

        let rejected = "from karva import fixture\n@fixture\ndef duplicate(): pass\n@fixture\ndef duplicate(): pass\ndef test_example(duplicate): pass\n";
        assert!(fixture_definition(&analysis(rejected), offset(rejected, "duplicate):")).is_none());
    }
}
