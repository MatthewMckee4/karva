#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "language-server occurrence consumers land in a later stack layer"
    )
)]

use ruff_python_ast::{Expr, StmtFunctionDef};
use ruff_text_size::{Ranged, TextRange, TextSize};

use crate::{FixtureId, FixtureResolution, SourceAnalysis};

/// The source construct containing a fixture occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FixtureOccurrenceKind {
    /// A fixture provider declaration.
    Definition,

    /// A fixture dependency parameter.
    Dependency,

    /// A test function parameter supplied by a fixture.
    TestParameter,

    /// A fixture name in `usefixtures` metadata.
    UseFixtures,
}

/// A source occurrence resolved to one fixture provider.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FixtureOccurrence {
    /// The occurrence's source range.
    range: TextRange,

    /// Range that can be replaced with another fixture name.
    ///
    /// This is `None` when implicit string concatenation prevents a safe
    /// single edit.
    edit_range: Option<TextRange>,

    /// The kind of source construct containing the occurrence.
    kind: FixtureOccurrenceKind,

    /// The fixture provider selected by static analysis.
    fixture: FixtureId,
}

/// Enumerates statically resolved fixture occurrences in the current source.
fn fixture_occurrences(analysis: &SourceAnalysis) -> Vec<FixtureOccurrence> {
    let mut occurrences = Vec::new();

    occurrences.extend(
        analysis
            .fixtures
            .iter()
            .map(|definition| FixtureOccurrence {
                range: definition.public_name_range,
                edit_range: definition.public_name_edit_range,
                kind: FixtureOccurrenceKind::Definition,
                fixture: definition.id.clone(),
            }),
    );

    occurrences.extend(analysis.fixtures.iter().flat_map(|definition| {
        definition.dependencies.iter().filter_map(|reference| {
            let FixtureResolution::Resolved(fixture) = &reference.resolution else {
                return None;
            };
            Some(FixtureOccurrence {
                range: reference.range,
                edit_range: Some(reference.range),
                kind: FixtureOccurrenceKind::Dependency,
                fixture: fixture.clone(),
            })
        })
    }));

    for function in &analysis.module.test_function_defs {
        occurrences.extend(function.parameters.iter_non_variadic_params().filter_map(
            |parameter| {
                let name = parameter.parameter.name.as_str();
                if !crate::fixture::test_parameter_is_fixture(&analysis.module, function, name) {
                    return None;
                }
                Some(FixtureOccurrence {
                    range: parameter.parameter.name.range,
                    edit_range: Some(parameter.parameter.name.range),
                    kind: FixtureOccurrenceKind::TestParameter,
                    fixture: resolve_source_fixture(analysis, name)?,
                })
            },
        ));
        occurrences.extend(use_fixtures_occurrences(analysis, function));
    }

    occurrences.sort_by_key(|occurrence| occurrence.range.start());
    occurrences
}

/// Returns the resolved fixture occurrence containing `offset`, if any.
fn fixture_occurrence(
    analysis: &SourceAnalysis,
    offset: TextSize,
) -> Option<FixtureOccurrence> {
    fixture_occurrences(analysis)
        .into_iter()
        .find(|occurrence| occurrence.range.contains_inclusive(offset))
}

fn resolve_source_fixture(analysis: &SourceAnalysis, name: &str) -> Option<FixtureId> {
    if analysis
        .visible_fixtures
        .iter()
        .any(|fixture| fixture.name == name)
        && !analysis.fixture_completion_blocked_names.contains(name)
    {
        return analysis
            .visible_fixtures
            .iter()
            .find(|fixture| fixture.name == name)
            .map(|fixture| fixture.id.clone());
    }
    None
}

fn use_fixtures_occurrences(
    analysis: &SourceAnalysis,
    function: &StmtFunctionDef,
) -> Vec<FixtureOccurrence> {
    function
        .decorator_list
        .iter()
        .filter_map(|decorator| {
            let Expr::Call(call) = &decorator.expression else {
                return None;
            };
            crate::fixture::is_use_fixtures_reference(&analysis.module, &call.func).then_some(call)
        })
        .flat_map(move |call| {
            call.arguments.args.iter().filter_map(move |argument| {
                let Expr::StringLiteral(literal) = argument else {
                    return None;
                };
                let edit_range = crate::fixture::single_string_content_range(literal);
                let range = edit_range.unwrap_or_else(|| literal.range());
                let name = literal.value.to_str();
                Some(FixtureOccurrence {
                    range,
                    edit_range,
                    kind: FixtureOccurrenceKind::UseFixtures,
                    fixture: resolve_source_fixture(analysis, name)?,
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
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

    fn at_last(source: &str, marker: &str) -> TextSize {
        TextSize::try_from(source.rfind(marker).expect("marker exists")).expect("source fits")
    }

    #[test]
    fn enumerates_declarations_dependencies_parameters_and_metadata() {
        let source = "import pytest\nfrom karva import fixture\n@fixture(name='database')\ndef provider(): pass\n@fixture\ndef wrapper(database): pass\n@pytest.mark.usefixtures('database')\ndef test_example(wrapper): pass\n";
        let occurrences = fixture_occurrences(&analysis(source));
        assert_eq!(
            occurrences
                .iter()
                .map(|occurrence| occurrence.kind)
                .collect::<Vec<_>>(),
            [
                FixtureOccurrenceKind::Definition,
                FixtureOccurrenceKind::Definition,
                FixtureOccurrenceKind::Dependency,
                FixtureOccurrenceKind::UseFixtures,
                FixtureOccurrenceKind::TestParameter,
            ]
        );
        assert_eq!(
            occurrences[0].range,
            TextRange::new(
                at(source, "database'"),
                at(source, "database'") + TextSize::from(8)
            )
        );
        assert_eq!(occurrences[0].fixture, occurrences[2].fixture);
        assert_eq!(occurrences[0].fixture, occurrences[3].fixture);
        assert_eq!(occurrences[0].edit_range, Some(occurrences[0].range));
    }

    #[test]
    fn excludes_builtins_missing_rejected_and_parametrized_parameters() {
        let source = "import pytest\nfrom karva import fixture\n@fixture\ndef database(): pass\n@pytest.mark.parametrize('value', [1])\ndef test_example(value, database, tmp_path, missing): pass\n";
        let occurrences = fixture_occurrences(&analysis(source));
        assert_eq!(
            occurrences
                .iter()
                .filter(|occurrence| occurrence.kind == FixtureOccurrenceKind::TestParameter)
                .count(),
            1
        );
        assert!(fixture_occurrence(&analysis(source), at(source, "tmp_path")).is_none());
    }

    #[test]
    fn custom_name_declaration_targets_string_not_python_name() {
        let source = "from karva import fixture\n@fixture(name=\"данные\")\ndef provider(): pass\n";
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
    }

    #[test]
    fn resolves_escaped_karva_use_fixtures_name() {
        let source = "import karva\nfrom karva import fixture\n@fixture\ndef database(): pass\n@karva.tags.use_fixtures(\"data\\x62ase\")\ndef test_example(): pass\n";
        let occurrence = fixture_occurrence(&analysis(source), at(source, "x62"))
            .expect("escaped use-fixtures occurrence");

        assert_eq!(occurrence.kind, FixtureOccurrenceKind::UseFixtures);
        assert_eq!(occurrence.fixture.path, "/project/test_example.py");
        assert_eq!(
            occurrence.range,
            TextRange::new(
                at(source, "data\\x62ase"),
                at(source, "data\\x62ase") + TextSize::from(11),
            )
        );
    }

    #[test]
    fn marks_implicitly_concatenated_names_uneditable() {
        let source = "import pytest\nfrom karva import fixture\n@fixture(name='data' 'base')\ndef provider(): pass\n@pytest.mark.usefixtures('data' 'base')\ndef test_example(database): pass\n";
        let occurrences = fixture_occurrences(&analysis(source));
        let definition = occurrences
            .iter()
            .find(|occurrence| occurrence.kind == FixtureOccurrenceKind::Definition)
            .expect("definition occurrence");
        let metadata = occurrences
            .iter()
            .find(|occurrence| occurrence.kind == FixtureOccurrenceKind::UseFixtures)
            .expect("metadata occurrence");

        assert_eq!(definition.fixture, metadata.fixture);
        assert_eq!(definition.edit_range, None);
        assert_eq!(metadata.edit_range, None);
        assert!(definition.range.contains(at(source, "'data' 'base'")));
        assert!(metadata.range.contains(at_last(source, "'data' 'base'")));
    }

    #[test]
    fn keeps_overridden_fixture_identities_separate() {
        let root_source = "from karva import fixture\n@fixture\ndef database(): pass\n";
        let nested_source = "from karva import fixture\n@fixture\ndef database(database): pass\n";
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

        let nested_analysis = analyze_source_with_parents(
            nested.clone(),
            [root.clone()],
            Utf8Path::new("/project"),
            &settings,
        )
        .expect("nested source should analyze");
        let test_analysis =
            analyze_source_with_parents(test, [root, nested], Utf8Path::new("/project"), &settings)
                .expect("test source should analyze");
        let nested_occurrences = fixture_occurrences(&nested_analysis);
        let declaration = nested_occurrences
            .iter()
            .find(|occurrence| occurrence.kind == FixtureOccurrenceKind::Definition)
            .expect("nested declaration");
        let dependency = nested_occurrences
            .iter()
            .find(|occurrence| occurrence.kind == FixtureOccurrenceKind::Dependency)
            .expect("outer dependency");
        let test_parameter = fixture_occurrences(&test_analysis)
            .into_iter()
            .find(|occurrence| occurrence.kind == FixtureOccurrenceKind::TestParameter)
            .expect("test parameter");

        assert_eq!(declaration.fixture.path, "/project/pkg/conftest.py");
        assert_eq!(dependency.fixture.path, "/project/conftest.py");
        assert_eq!(test_parameter.fixture, declaration.fixture);
        assert_ne!(declaration.fixture, dependency.fixture);
    }
}
