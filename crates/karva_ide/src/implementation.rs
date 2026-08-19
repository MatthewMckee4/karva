use camino::Utf8PathBuf;
use ruff_text_size::{TextRange, TextSize};

use crate::{SourceAnalysis, fixture_target};

/// A source location for a fixture's implementation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureImplementationTarget {
    /// File containing the fixture provider.
    pub path: Utf8PathBuf,

    /// UTF-8 byte range of the first yield expression, or the provider name
    /// for non-generator fixtures.
    pub range: TextRange,
}

/// Resolves a fixture reference to the provider's implementation.
///
/// This follows the exact source-provider identity selected by fixture
/// resolution, including providers inherited from parent configuration
/// modules. Built-ins, rejected definitions, missing providers, and dynamic
/// providers do not produce a target.
pub fn fixture_implementation(
    analysis: &SourceAnalysis,
    offset: TextSize,
) -> Option<FixtureImplementationTarget> {
    let fixture = fixture_target(analysis, offset)?;
    let definition = analysis.fixture_model.definition(&fixture)?;
    Some(FixtureImplementationTarget {
        path: definition.id.path.clone(),
        range: definition.implementation_range,
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

    fn range(source: &str, marker: &str) -> TextRange {
        let start = offset(source, marker);
        TextRange::new(
            start,
            start + TextSize::try_from(marker.len()).expect("source fits"),
        )
    }

    fn assert_target(source: &str, reference: &str, implementation: &str) {
        let target = fixture_implementation(&analysis(source), offset(source, reference))
            .expect("implementation target");
        assert_eq!(target.path, "/project/test_example.py");
        assert_eq!(target.range, range(source, implementation));
    }

    #[test]
    fn resolves_regular_fixture_to_function_name() {
        assert_target(
            "from karva import fixture\n@fixture\ndef database(): pass\ndef test_example(database): pass\n",
            "database):",
            "database",
        );
    }

    #[test]
    fn resolves_return_fixture_to_function_name() {
        assert_target(
            "from karva import fixture\n@fixture\ndef database():\n    return object()\ndef test_example(database): pass\n",
            "database):",
            "database",
        );
    }

    #[test]
    fn resolves_first_yield_in_source_order() {
        let source = "from karva import fixture\n@fixture\ndef database():\n    if ready:\n        yield first\n    yield second\ndef test_example(database): pass\n";
        assert_target(source, "database):", "yield first");
    }

    #[test]
    fn resolves_yield_from() {
        let source = "from karva import fixture\n@fixture\ndef database():\n    yield from values\ndef test_example(database): pass\n";
        assert_target(source, "database):", "yield from values");
    }

    #[test]
    fn ignores_nested_functions_and_classes() {
        let source = "from karva import fixture\n@fixture\ndef database():\n    def nested():\n        yield hidden\n    class Nested:\n        def method(self):\n            yield hidden\n    nested_lambda = lambda: (yield hidden)\n    return value\ndef test_example(database): pass\n";
        assert_target(source, "database):", "database");
    }

    #[test]
    fn resolves_dependency_and_usefixtures_references() {
        let source = "import pytest\nfrom karva import fixture\n@fixture\ndef database():\n    yield value\n@fixture\ndef wrapper(database): pass\n@pytest.mark.usefixtures(\"database\")\ndef test_example(wrapper): pass\n";
        let result = analysis(source);
        assert_eq!(
            fixture_implementation(&result, offset(source, "database):"))
                .expect("dependency target")
                .range,
            range(source, "yield value")
        );
        assert_eq!(
            fixture_implementation(&result, offset_last(source, "database\""))
                .expect("usefixtures target")
                .range,
            range(source, "yield value")
        );
    }

    #[test]
    fn resolves_inherited_provider() {
        let parent_source =
            "from karva import fixture\n@fixture\ndef database():\n    yield value\n";
        let source = "def test_example(database): pass\n";
        let analysis = analyze_source_with_parents(
            SourceDocument::new(
                Utf8PathBuf::from("/project/test_example.py"),
                source.to_owned(),
            ),
            [SourceDocument::new(
                Utf8PathBuf::from("/project/conftest.py"),
                parent_source.to_owned(),
            )],
            Utf8Path::new("/project"),
            &settings(),
        )
        .expect("source should analyze");
        let target = fixture_implementation(&analysis, offset(source, "database)"))
            .expect("inherited target");
        assert_eq!(target.path, "/project/conftest.py");
        assert_eq!(target.range, range(parent_source, "yield value"));
    }

    #[test]
    fn resolves_nearest_override() {
        let parent_source =
            "from karva import fixture\n@fixture\ndef database():\n    yield parent\n";
        let source = "from karva import fixture\n@fixture\ndef database():\n    yield child\ndef test_example(database): pass\n";
        let analysis = analyze_source_with_parents(
            SourceDocument::new(
                Utf8PathBuf::from("/project/test_example.py"),
                source.to_owned(),
            ),
            [SourceDocument::new(
                Utf8PathBuf::from("/project/conftest.py"),
                parent_source.to_owned(),
            )],
            Utf8Path::new("/project"),
            &settings(),
        )
        .expect("source should analyze");
        let target = fixture_implementation(&analysis, offset(source, "database):"))
            .expect("override target");
        assert_eq!(target.path, "/project/test_example.py");
        assert_eq!(target.range, range(source, "yield child"));
    }

    #[test]
    fn suppresses_builtin_missing_and_rejected_targets() {
        let builtin = analysis("def test_example(tmp_path): pass\n");
        assert!(
            fixture_implementation(&builtin, offset(&builtin.module.source_text, "tmp_path"))
                .is_none()
        );

        let missing = analysis("def test_example(missing): pass\n");
        assert!(
            fixture_implementation(&missing, offset(&missing.module.source_text, "missing"))
                .is_none()
        );

        let rejected = analysis(
            "from karva import fixture\n@fixture\ndef duplicate(): pass\n@fixture\ndef duplicate(): pass\ndef test_example(duplicate): pass\n",
        );
        assert!(
            fixture_implementation(
                &rejected,
                offset(&rejected.module.source_text, "duplicate):")
            )
            .is_none()
        );
    }
}
