//! Direct fixture dependency edges, using the same provider identities as references.

use camino::Utf8PathBuf;
use ruff_python_ast::StmtFunctionDef;
use ruff_text_size::{Ranged, TextRange};

use crate::{
    FixtureId, FixtureOccurrenceKind, SourceAnalysis, SourceSymbolKind, WorkspaceSourceIndex,
    fixture_occurrences, fixture_target,
};

/// A fixture or test function that can participate in the fixture graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureHierarchyItem {
    /// File containing the function.
    pub path: Utf8PathBuf,

    /// Python function name displayed by the editor.
    pub name: String,

    /// Distinguishes fixture dependency navigation from ordinary Python calls.
    pub kind: SourceSymbolKind,

    /// Complete function range, including decorators.
    pub range: TextRange,

    /// Function name selected when navigating to the item.
    pub selection_range: TextRange,
}

/// One direct dependency edge grouped by the function at its other end.
#[derive(Debug)]
pub struct FixtureHierarchyCall {
    /// Consumer for incoming calls, provider for outgoing calls.
    pub item: FixtureHierarchyItem,

    /// Reference ranges in the consumer file, in source order.
    pub ranges: Vec<TextRange>,
}

/// Returns accepted fixture and collected test functions in source order.
pub fn fixture_hierarchy_items(analysis: &SourceAnalysis) -> Vec<FixtureHierarchyItem> {
    let mut items = analysis
        .module
        .test_function_defs
        .iter()
        .chain(
            analysis
                .module
                .fixture_function_defs
                .iter()
                .filter(|function| {
                    analysis
                        .fixture_model
                        .local()
                        .iter()
                        .any(|definition| definition.id.range == function.name.range)
                }),
        )
        .map(|function| item(analysis, function))
        .collect::<Vec<_>>();
    items.sort_by_key(|item| item.range.start());
    items
}

/// Finds the declaration for a statically resolved provider, including inherited fixtures.
pub fn fixture_hierarchy_provider(
    index: &WorkspaceSourceIndex,
    target: &FixtureId,
) -> Option<FixtureHierarchyItem> {
    let analysis = index.analyze(&target.path)?;
    fixture_hierarchy_items(&analysis).into_iter().find(|item| {
        item.selection_range == target.range
            && fixture_target(&analysis, item.selection_range.start()).as_ref() == Some(target)
    })
}

/// Finds tests and fixture parameters resolving to this exact provider.
///
/// No graph traversal is performed, so cycles remain finite direct edges.
pub fn fixture_incoming_calls(
    index: &WorkspaceSourceIndex,
    target: &FixtureId,
) -> Vec<FixtureHierarchyCall> {
    let mut calls = Vec::new();
    for path in index.paths() {
        let Some(analysis) = index.analyze(path) else {
            continue;
        };
        let items = fixture_hierarchy_items(&analysis);
        for occurrence in fixture_occurrences(&analysis) {
            if !matches!(
                occurrence.kind,
                FixtureOccurrenceKind::Dependency
                    | FixtureOccurrenceKind::TestParameter
                    | FixtureOccurrenceKind::UseFixtures
            ) || &occurrence.fixture != target
            {
                continue;
            }
            if let Some(consumer) = items
                .iter()
                .find(|item| item.range.contains_range(occurrence.range))
            {
                add_call(&mut calls, consumer.clone(), occurrence.range);
            }
        }
    }
    calls
}

/// Finds direct source fixture dependencies of a fixture or test function.
///
/// Built-in, missing, rejected, and dynamic references have no source edge.
pub fn fixture_outgoing_calls(
    index: &WorkspaceSourceIndex,
    consumer: &FixtureHierarchyItem,
) -> Vec<FixtureHierarchyCall> {
    let Some(analysis) = index.analyze(&consumer.path) else {
        return Vec::new();
    };
    if !fixture_hierarchy_items(&analysis).contains(consumer) {
        return Vec::new();
    }
    let mut calls = Vec::new();
    for occurrence in fixture_occurrences(&analysis) {
        if !matches!(
            occurrence.kind,
            FixtureOccurrenceKind::Dependency
                | FixtureOccurrenceKind::TestParameter
                | FixtureOccurrenceKind::UseFixtures
        ) || !consumer.range.contains_range(occurrence.range)
        {
            continue;
        }
        if let Some(provider) = fixture_hierarchy_provider(index, &occurrence.fixture) {
            add_call(&mut calls, provider, occurrence.range);
        }
    }
    calls
}

fn item(analysis: &SourceAnalysis, function: &StmtFunctionDef) -> FixtureHierarchyItem {
    FixtureHierarchyItem {
        path: analysis.module.path.path().to_path_buf(),
        name: function.name.to_string(),
        kind: if analysis
            .fixture_model
            .local()
            .iter()
            .any(|definition| definition.id.range == function.name.range)
        {
            SourceSymbolKind::Fixture
        } else {
            SourceSymbolKind::Test
        },
        range: TextRange::new(
            function
                .decorator_list
                .first()
                .map_or_else(|| function.start(), Ranged::start),
            function.end(),
        ),
        selection_range: function.name.range,
    }
}

fn add_call(calls: &mut Vec<FixtureHierarchyCall>, item: FixtureHierarchyItem, range: TextRange) {
    if let Some(call) = calls.iter_mut().find(|call| call.item == item) {
        if !call.ranges.contains(&range) {
            call.ranges.push(range);
        }
    } else {
        calls.push(FixtureHierarchyCall {
            item,
            ranges: vec![range],
        });
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8Path;
    use ruff_python_ast::PythonVersion;
    use ruff_text_size::TextSize;

    use super::*;
    use crate::{SourceAnalysisSettings, SourceDocument};

    fn index(sources: &[(&str, &str)]) -> WorkspaceSourceIndex {
        WorkspaceSourceIndex::from_documents(
            "/project".into(),
            sources.iter().map(|(path, source)| {
                SourceDocument::new(Utf8Path::new("/project").join(path), (*source).to_owned())
            }),
            SourceAnalysisSettings {
                python_version: PythonVersion::PY312,
                test_function_prefix: "test".to_owned(),
                try_import_fixtures: false,
            },
        )
        .expect("sources index")
    }

    fn target(index: &WorkspaceSourceIndex, path: &str, marker: &str) -> FixtureId {
        let analysis = index
            .analyze(&Utf8Path::new("/project").join(path))
            .expect("source analysis");
        fixture_target(
            &analysis,
            TextSize::try_from(analysis.module.source_text.find(marker).expect("marker"))
                .expect("offset"),
        )
        .expect("provider")
    }

    #[test]
    fn groups_consumers_and_preserves_override_identity() {
        let index = index(&[
            (
                "conftest.py",
                "from karva import fixture\n@fixture\ndef database(): pass\n",
            ),
            (
                "pkg/conftest.py",
                "from karva import fixture\n@fixture\ndef database(database): pass\n",
            ),
            (
                "test_root.py",
                "import pytest\n@pytest.mark.usefixtures('database')\ndef test_root(database): pass\n",
            ),
            ("pkg/test_nested.py", "def test_nested(database): pass\n"),
        ]);
        let root = target(&index, "conftest.py", "database");
        let calls = fixture_incoming_calls(&index, &root);
        assert_eq!(
            calls
                .iter()
                .map(|call| (call.item.name.as_str(), call.ranges.len()))
                .collect::<Vec<_>>(),
            [("database", 1), ("test_root", 2)]
        );
        for call in &calls {
            assert!(
                call.ranges
                    .iter()
                    .all(|range| call.item.range.contains_range(*range))
            );
        }
        let nested = target(&index, "pkg/test_nested.py", "database");
        let calls = fixture_incoming_calls(&index, &nested);
        assert_eq!(
            calls
                .iter()
                .map(|call| call.item.name.as_str())
                .collect::<Vec<_>>(),
            ["test_nested"]
        );
        let provider = fixture_hierarchy_provider(&index, &nested).expect("nested provider");
        let outgoing = fixture_outgoing_calls(&index, &provider);
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].item.path, root.path);
        assert_eq!(outgoing[0].item.selection_range, root.range);
    }

    #[test]
    fn cycles_are_direct_edges_and_ordinary_python_calls_are_excluded() {
        let index = index(&[(
            "test_cycle.py",
            "from karva import fixture\n@fixture\ndef first(second):\n    return second\n@fixture\ndef second(first):\n    first()\n    return first\ndef test_example(first):\n    second()\n",
        )]);
        let first = target(&index, "test_cycle.py", "first(");
        let item = fixture_hierarchy_provider(&index, &first).expect("first provider");
        let outgoing = fixture_outgoing_calls(&index, &item);
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].item.name, "second");
        assert_eq!(outgoing[0].ranges.len(), 1);
        let incoming = fixture_incoming_calls(&index, &first);
        assert_eq!(
            incoming
                .iter()
                .map(|call| call.item.name.as_str())
                .collect::<Vec<_>>(),
            ["second", "test_example"]
        );
        assert!(incoming.iter().all(|call| call.ranges.len() == 1));
    }

    #[test]
    fn custom_names_and_builtin_or_rejected_dependencies() {
        let index = index(&[(
            "test_example.py",
            "from karva import fixture\n@fixture(name='данные')\ndef provider(): pass\n@fixture(scope='invalid')\ndef rejected(): pass\n@fixture\ndef wrapper(данные, tmp_path, rejected, missing): pass\ndef test_example(wrapper): pass\n",
        )]);
        let wrapper = target(&index, "test_example.py", "wrapper(");
        let outgoing = fixture_outgoing_calls(
            &index,
            &fixture_hierarchy_provider(&index, &wrapper).expect("wrapper"),
        );
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].item.name, "provider");
        let analysis = index
            .analyze(Utf8Path::new("/project/test_example.py"))
            .expect("analysis");
        assert_eq!(
            &analysis.module.source_text[outgoing[0].ranges[0].to_std_range()],
            "данные"
        );
        assert!(
            fixture_target(
                &analysis,
                TextSize::try_from(
                    analysis
                        .module
                        .source_text
                        .find("tmp_path")
                        .expect("builtin")
                )
                .expect("offset")
            )
            .is_none()
        );
        assert!(
            !fixture_hierarchy_items(&analysis)
                .iter()
                .any(|item| item.name == "rejected")
        );
    }
}
