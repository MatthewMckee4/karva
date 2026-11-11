use ruff_text_size::TextSize;

#[cfg(test)]
use ruff_text_size::TextRange;

use crate::{FixtureId, FixtureOccurrence, FixtureOccurrenceKind, SourceAnalysis};
use karva_analysis::{fixture_occurrence, fixture_occurrences};

/// Fixture selected for rename plus its public-name placeholder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureRenameTarget {
    /// Resolved fixture selection under the cursor.
    pub occurrence: FixtureOccurrence,

    /// Public fixture name shown by rename-capable clients.
    pub placeholder: String,
}

/// Resolves the fixture symbol under `offset` to its provider identity.
///
/// Custom public fixture names remain addressable from both the decorator
/// string and the provider function name used by definition navigation.
pub fn fixture_target(analysis: &SourceAnalysis, offset: TextSize) -> Option<FixtureId> {
    fixture_occurrence(analysis, offset)
        .map(|occurrence| occurrence.fixture)
        .or_else(|| {
            analysis
                .fixture_model()
                .local()
                .iter()
                .find(|definition| definition.name_range.contains_inclusive(offset))
                .map(|definition| definition.id.clone())
        })
}

/// Returns fixture highlights in the current document for the provider under `offset`.
///
/// Fixture declarations and custom provider function names are writes. Fixture
/// dependencies, test parameters, and `usefixtures` metadata are reads.
pub fn fixture_document_highlights(
    analysis: &SourceAnalysis,
    offset: TextSize,
) -> Option<Vec<FixtureOccurrence>> {
    let fixture = fixture_target(analysis, offset)?;
    let mut highlights: Vec<_> = fixture_occurrences(analysis)
        .into_iter()
        .filter(|occurrence| occurrence.fixture == fixture)
        .collect();

    if let Some(definition) = analysis
        .fixture_model()
        .local()
        .iter()
        .find(|definition| definition.id == fixture)
        && definition.name_range != definition.public_name_range
    {
        highlights.push(FixtureOccurrence {
            range: definition.name_range,
            edit_range: None,
            kind: FixtureOccurrenceKind::Definition,
            fixture,
        });
    }
    highlights.sort_by_key(|occurrence| occurrence.range.start());
    Some(highlights)
}

/// Resolves a rename selection, including custom fixture provider anchors.
///
/// For `@fixture(name="public") def provider`, definition navigation lands on
/// `provider`. Rename still edits only the public decorator name and its
/// references, while the placeholder tells the client which name is changing.
pub fn fixture_rename_target(
    analysis: &SourceAnalysis,
    offset: TextSize,
) -> Option<FixtureRenameTarget> {
    if let Some(occurrence) = fixture_occurrence(analysis, offset) {
        let placeholder = fixture_name(analysis, &occurrence.fixture)?;
        return Some(FixtureRenameTarget {
            occurrence,
            placeholder,
        });
    }

    let definition = analysis
        .fixture_model()
        .local()
        .iter()
        .find(|definition| definition.name_range.contains_inclusive(offset))?;
    Some(FixtureRenameTarget {
        occurrence: FixtureOccurrence {
            range: definition.name_range,
            edit_range: definition.public_name_edit_range,
            kind: FixtureOccurrenceKind::Definition,
            fixture: definition.id.clone(),
        },
        placeholder: definition.name.clone(),
    })
}

fn fixture_name(analysis: &SourceAnalysis, fixture: &FixtureId) -> Option<String> {
    analysis
        .fixture_model()
        .definition(fixture)
        .map(|definition| definition.name.clone())
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use camino::{Utf8Path, Utf8PathBuf};
    use ruff_python_ast::PythonVersion;
    use ruff_text_size::TextSize;

    use super::*;
    use crate::{
        SourceAnalysisSettings, SourceDocument, analyze_source, analyze_source_with_parents,
    };

    fn analysis(source: &str) -> crate::SourceAnalysis {
        analyze_source(
            &Utf8PathBuf::from("/project/test_example.py"),
            Utf8Path::new("/project"),
            source.to_owned(),
            &SourceAnalysisSettings {
                python_version: PythonVersion::PY312,
                test_function_prefix: "test".to_owned(),
                try_import_fixtures: false,
            },
        )
        .expect("source should analyze")
    }

    fn at(source: &str, marker: &str) -> TextSize {
        TextSize::try_from(source.find(marker).expect("marker exists")).expect("source fits")
    }

    #[test]
    fn custom_name_declaration_targets_string_not_python_name() {
        let source = indoc! {r#"
            from karva import fixture
            @fixture(name="данные")
            def provider(): pass
        "#};
        let occurrence = fixture_occurrences(&analysis(source))
            .into_iter()
            .find(|occurrence| occurrence.kind == FixtureOccurrenceKind::Definition)
            .expect("declaration occurrence");
        assert_eq!(
            occurrence.range,
            TextRange::new(
                at(source, "данные\""),
                at(source, "данные\"") + TextSize::from(12)
            )
        );
        assert!(fixture_occurrence(&analysis(source), at(source, "provider")).is_none());
        assert_eq!(
            fixture_target(&analysis(source), at(source, "provider")).map(|fixture| fixture.path),
            Some("/project/test_example.py".into())
        );
        let rename = fixture_rename_target(&analysis(source), at(source, "provider"))
            .expect("custom provider should be a rename target");
        assert_eq!(rename.placeholder, "данные");
        assert_eq!(&source[rename.occurrence.range.to_std_range()], "provider");
        let edit_range = rename
            .occurrence
            .edit_range
            .expect("custom public name should be editable");
        assert_eq!(&source[edit_range.to_std_range()], "данные");
    }

    #[test]
    fn highlights_custom_provider_anchor_and_consumers() {
        let source = indoc! {r#"
            from karva import fixture
            @fixture(name="database")
            def provider(): pass
            def test_example(database): pass
        "#};
        let highlights = fixture_document_highlights(&analysis(source), at(source, "provider"))
            .expect("custom fixture should highlight");

        assert_eq!(
            highlights
                .iter()
                .map(|occurrence| occurrence.kind)
                .collect::<Vec<_>>(),
            [
                FixtureOccurrenceKind::Definition,
                FixtureOccurrenceKind::Definition,
                FixtureOccurrenceKind::TestParameter,
            ]
        );
        assert_eq!(
            highlights[0].range,
            TextRange::at(at(source, "database"), TextSize::from(8)),
        );
        assert_eq!(
            highlights[1].range,
            TextRange::at(at(source, "provider"), TextSize::from(8)),
        );
    }

    #[test]
    fn highlights_only_the_selected_nested_provider() {
        let root_source = indoc! {r"
            from karva import fixture
            @fixture
            def database(): pass
        "};
        let nested_source = indoc! {r"
            from karva import fixture
            @fixture
            def database(): pass
        "};
        let test_source = "def test_example(database): pass\n";
        let root = SourceDocument::new(
            Utf8PathBuf::from("/project/conftest.py"),
            root_source.to_owned(),
        );
        let nested = SourceDocument::new(
            Utf8PathBuf::from("/project/pkg/conftest.py"),
            nested_source.to_owned(),
        );
        let test = SourceDocument::new(
            Utf8PathBuf::from("/project/pkg/test_example.py"),
            test_source.to_owned(),
        );
        let settings = SourceAnalysisSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test".to_owned(),
            try_import_fixtures: false,
        };
        let analysis =
            analyze_source_with_parents(test, [root, nested], Utf8Path::new("/project"), &settings)
                .expect("test source should analyze");

        let highlights = fixture_document_highlights(&analysis, at(test_source, "database"))
            .expect("nested fixture should highlight");
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].kind, FixtureOccurrenceKind::TestParameter);
    }

    #[test]
    fn highlights_inherited_fixture_consumers_without_local_definition() {
        let root = SourceDocument::new(
            Utf8PathBuf::from("/project/conftest.py"),
            indoc! {r"
                from karva import fixture
                @fixture
                def database(): pass
            "}
            .to_owned(),
        );
        let test = SourceDocument::new(
            Utf8PathBuf::from("/project/test_example.py"),
            "def test_example(database): pass\n".to_owned(),
        );
        let settings = SourceAnalysisSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test".to_owned(),
            try_import_fixtures: false,
        };
        let analysis =
            analyze_source_with_parents(test, [root], Utf8Path::new("/project"), &settings)
                .expect("test source should analyze");

        let highlights = fixture_document_highlights(&analysis, TextSize::from(20))
            .expect("inherited fixture should highlight");
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].kind, FixtureOccurrenceKind::TestParameter);
    }
}
