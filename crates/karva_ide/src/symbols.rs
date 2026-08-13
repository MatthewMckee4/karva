use karva_collector::count_parametrize_cases;
use ruff_text_size::{Ranged, TextRange};

use crate::SourceAnalysis;

/// Kind of source symbol exposed by Karva's editor analysis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceSymbolKind {
    /// A top-level collected test function.
    Test,

    /// A statically accepted local fixture provider.
    Fixture,
}

/// A source symbol independent of editor protocol types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSymbol {
    /// Symbol kind.
    pub kind: SourceSymbolKind,

    /// Symbol name shown by the editor.
    pub name: String,

    /// Complete source range of the function definition.
    pub range: TextRange,

    /// Source range selecting the symbol's name.
    pub selection_range: TextRange,

    /// Optional human-readable symbol detail.
    pub detail: Option<String>,
}

/// Returns collected top-level tests and accepted local fixtures in source order.
pub fn source_symbols(analysis: &SourceAnalysis) -> Vec<SourceSymbol> {
    let mut symbols = analysis
        .module
        .test_function_defs
        .iter()
        .map(|function| SourceSymbol {
            kind: SourceSymbolKind::Test,
            name: function.name.to_string(),
            range: function.range(),
            selection_range: function.name.range,
            detail: count_parametrize_cases(function).map(|count| {
                if count == 1 {
                    "1 case".to_owned()
                } else {
                    format!("{count} cases")
                }
            }),
        })
        .chain(
            analysis
                .module
                .fixture_function_defs
                .iter()
                .filter_map(|function| {
                    let definition = analysis.fixtures.iter().find(|definition| {
                        definition.id.path.as_path() == analysis.module.path.path().as_path()
                            && definition.id.range == function.name.range
                    })?;
                    Some(SourceSymbol {
                        kind: SourceSymbolKind::Fixture,
                        name: definition.name.clone(),
                        range: function.range(),
                        selection_range: definition.public_name_range,
                        detail: None,
                    })
                }),
        )
        .collect::<Vec<_>>();
    symbols.sort_by_key(|symbol| symbol.range.start());
    symbols
}

#[cfg(test)]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use ruff_python_ast::PythonVersion;
    use ruff_text_size::{TextRange, TextSize};

    use super::*;
    use crate::{SourceAnalysisSettings, analyze_source};

    fn settings(prefix: &str) -> SourceAnalysisSettings {
        SourceAnalysisSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: prefix.to_owned(),
            try_import_fixtures: false,
        }
    }

    fn symbols(source: &str, prefix: &str) -> Vec<SourceSymbol> {
        let analysis = analyze_source(
            &Utf8PathBuf::from("/project/test_example.py"),
            Utf8Path::new("/project"),
            source.to_owned(),
            &settings(prefix),
        )
        .expect("source should analyze");
        source_symbols(&analysis)
    }

    fn range(source: &str, marker: &str) -> TextRange {
        let start =
            TextSize::try_from(source.find(marker).expect("marker exists")).expect("source fits");
        TextRange::new(
            start,
            start + TextSize::try_from(marker.len()).expect("marker fits"),
        )
    }

    #[test]
    fn emits_regular_and_custom_fixtures_with_public_names() {
        let source = "from karva import fixture\n\n@fixture\ndef database(): pass\n\n@fixture(name=\"store\")\ndef provider(): pass\n";
        let symbols = symbols(source, "test");

        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].kind, SourceSymbolKind::Fixture);
        assert_eq!(symbols[0].name, "database");
        assert_eq!(
            symbols[0].range,
            range(source, "@fixture\ndef database(): pass")
        );
        assert_eq!(symbols[0].selection_range, range(source, "database"));
        assert_eq!(symbols[0].detail, None);
        assert_eq!(symbols[1].name, "store");
        assert_eq!(
            symbols[1].range,
            range(source, "@fixture(name=\"store\")\ndef provider(): pass")
        );
        assert_eq!(symbols[1].selection_range, range(source, "store"));
    }

    #[test]
    fn emits_tests_with_static_case_detail_and_no_dynamic_detail() {
        let source = "import pytest\n\n@pytest.mark.parametrize(\"value\", [1, 2])\ndef test_static(value): pass\n\n@pytest.mark.parametrize(\"value\", cases())\ndef test_dynamic(value): pass\n";
        let symbols = symbols(source, "test");

        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].kind, SourceSymbolKind::Test);
        assert_eq!(symbols[0].name, "test_static");
        assert_eq!(symbols[0].selection_range, range(source, "test_static"));
        assert_eq!(symbols[0].detail.as_deref(), Some("2 cases"));
        assert_eq!(symbols[1].name, "test_dynamic");
        assert_eq!(symbols[1].detail, None);
    }

    #[test]
    fn uses_singular_case_detail() {
        let source = "import pytest\n\n@pytest.mark.parametrize(\"value\", [1])\ndef test_one(value): pass\n";

        assert_eq!(symbols(source, "test")[0].detail.as_deref(), Some("1 case"));
    }

    #[test]
    fn excludes_rejected_fixtures_and_uses_custom_test_prefix() {
        let source = "from karva import fixture\n\n@fixture\ndef duplicate(): pass\n@fixture\ndef duplicate(): pass\n\ndef spec_case(): pass\ndef test_case(): pass\n";
        let symbols = symbols(source, "spec_");

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].kind, SourceSymbolKind::Test);
        assert_eq!(symbols[0].name, "spec_case");
    }

    #[test]
    fn symbols_follow_source_order() {
        let source = "from karva import fixture\n\ndef test_before(): pass\n\n@fixture\ndef database(): pass\n\ndef test_after(): pass\n";
        let symbols = symbols(source, "test");

        assert_eq!(
            symbols
                .iter()
                .map(|symbol| symbol.name.as_str())
                .collect::<Vec<_>>(),
            ["test_before", "database", "test_after"]
        );
    }
}
