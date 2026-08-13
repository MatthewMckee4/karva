use karva_collector::{ModuleType, collect_doctests, count_parametrize_cases};
use ruff_text_size::{Ranged, TextRange};

use crate::SourceAnalysis;

/// Kind of test target exposed by Karva's editor analysis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceTestTargetKind {
    /// A top-level test function and its statically discoverable case count.
    Function { case_count: Option<usize> },

    /// One docstring containing executable examples.
    Doctest,
}

/// Exact runtime target found in one source document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceTestTarget {
    /// Selector suffix accepted after `<path>::`.
    pub name: String,

    /// Complete range represented by the target.
    pub range: TextRange,

    /// Range an editor should highlight for the target.
    pub selection_range: TextRange,

    /// Runtime target kind.
    pub kind: SourceTestTargetKind,
}

/// Returns executable targets in source order.
pub fn source_test_targets(
    analysis: &SourceAnalysis,
    include_doctests: bool,
) -> Vec<SourceTestTarget> {
    let mut targets = analysis
        .module
        .test_function_defs
        .iter()
        .map(|function| SourceTestTarget {
            name: function.name.to_string(),
            range: function.range(),
            selection_range: function.name.range,
            kind: SourceTestTargetKind::Function {
                case_count: count_parametrize_cases(function),
            },
        })
        .collect::<Vec<_>>();

    if include_doctests && analysis.module.module_type() == ModuleType::Test {
        targets.extend(
            collect_doctests(&analysis.module.module_body, &analysis.module.source_text)
                .into_iter()
                .map(|doctest| SourceTestTarget {
                    name: doctest.name,
                    range: doctest.range,
                    selection_range: doctest.range,
                    kind: SourceTestTargetKind::Doctest,
                }),
        );
    }

    targets.sort_by_key(|target| target.range.start());
    targets
}

#[cfg(test)]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use ruff_python_ast::PythonVersion;

    use super::*;
    use crate::{SourceAnalysisSettings, analyze_source};

    fn targets(source: &str, include_doctests: bool) -> Vec<SourceTestTarget> {
        let analysis = analyze_source(
            &Utf8PathBuf::from("/project/test_example.py"),
            Utf8Path::new("/project"),
            source.to_owned(),
            &SourceAnalysisSettings {
                python_version: PythonVersion::PY312,
                test_function_prefix: "test".to_owned(),
                try_import_fixtures: false,
            },
        )
        .expect("source should analyze");
        source_test_targets(&analysis, include_doctests)
    }

    #[test]
    fn returns_functions_cases_and_doctests_in_source_order() {
        let targets = targets(
            "\"\"\">>> 1 + 1\n2\n\"\"\"\n\n@karva.tags.parametrize(\"value\", [1, 2])\ndef test_example(value): pass\n",
            true,
        );

        assert_eq!(
            targets
                .iter()
                .map(|target| (target.name.as_str(), target.kind))
                .collect::<Vec<_>>(),
            [
                ("doctest:@module", SourceTestTargetKind::Doctest),
                (
                    "test_example",
                    SourceTestTargetKind::Function {
                        case_count: Some(2)
                    }
                ),
            ]
        );
    }

    #[test]
    fn excludes_doctests_when_disabled() {
        assert!(targets("\"\"\">>> 1 + 1\n2\n\"\"\"\n", false).is_empty());
    }
}
