use ruff_text_size::TextRange;

use crate::{
    FixtureId, FixtureOccurrence, FixtureOccurrenceKind, LocatedFixtureOccurrence, SourceAnalysis,
    WorkspaceSourceIndex, fixture_occurrences,
};

/// Returns whether `name` is a valid public name for a fixture rename.
///
/// Renames can update Python parameters, so every new name must be a
/// non-keyword Python identifier even when the selected occurrence is a
/// decorator string.
pub fn is_valid_fixture_name(name: &str) -> bool {
    ruff_python_stdlib::identifiers::is_identifier(name)
}

/// Returns the current occurrence range when a fixture can be renamed safely.
///
/// Every occurrence resolving to the same provider must have one complete edit
/// range. Implicitly concatenated string literals therefore disable the whole
/// rename instead of producing a partial workspace edit.
pub fn prepare_fixture_rename(
    index: &WorkspaceSourceIndex,
    current: &FixtureOccurrence,
) -> Option<TextRange> {
    current.edit_range?;
    editable_fixture_occurrences(index, current, None).map(|_| current.range)
}

/// Returns every edit needed to rename one fixture provider.
///
/// Invalid Python identifiers and targets with any uneditable occurrence are
/// rejected before edits are returned.
pub fn rename_fixture(
    index: &WorkspaceSourceIndex,
    current: &FixtureOccurrence,
    new_name: &str,
) -> Option<Vec<LocatedFixtureOccurrence>> {
    if !is_valid_fixture_name(new_name) {
        return None;
    }
    editable_fixture_occurrences(index, current, Some(new_name))
}

fn editable_fixture_occurrences(
    index: &WorkspaceSourceIndex,
    current: &FixtureOccurrence,
    new_name: Option<&str>,
) -> Option<Vec<LocatedFixtureOccurrence>> {
    let mut occurrences = Vec::new();
    for path in index.paths() {
        let analysis = index.analyze(path)?;
        let matching = fixture_occurrences(&analysis)
            .into_iter()
            .filter(|occurrence| occurrence.fixture == current.fixture)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            continue;
        }
        if matching
            .iter()
            .any(|occurrence| occurrence.edit_range.is_none())
            || new_name.is_some_and(|new_name| {
                fixture_name_conflicts(&analysis, &matching, &current.fixture, new_name)
            })
        {
            return None;
        }
        occurrences.extend(
            matching
                .into_iter()
                .map(|occurrence| LocatedFixtureOccurrence {
                    path: path.to_path_buf(),
                    occurrence,
                }),
        );
    }
    (!occurrences.is_empty()).then_some(occurrences)
}

fn fixture_name_conflicts(
    analysis: &SourceAnalysis,
    occurrences: &[FixtureOccurrence],
    target: &FixtureId,
    new_name: &str,
) -> bool {
    let target_name = analysis
        .fixtures
        .iter()
        .chain(&analysis.visible_fixtures)
        .find(|definition| &definition.id == target)
        .map(|definition| definition.name.as_str());
    if target_name == Some(new_name) {
        return false;
    }

    analysis.fixture_completion_blocked_names.contains(new_name)
        || analysis
            .visible_fixtures
            .iter()
            .any(|definition| definition.name == new_name && &definition.id != target)
        || crate::fixture::builtin_info(new_name).is_some()
        || target_name.is_some_and(|target_name| {
            analysis
                .visible_fixtures
                .iter()
                .any(|definition| &definition.id == target && definition.name == target_name)
                && analysis.module.test_function_defs.iter().any(|function| {
                    crate::fixture::test_parametrization_is_dynamic(&analysis.module, function)
                        && function
                            .parameters
                            .iter_non_variadic_params()
                            .any(|parameter| parameter.parameter.name.as_str() == target_name)
                })
        })
        || analysis
            .module
            .test_function_defs
            .iter()
            .map(|function| (function, true))
            .chain(
                analysis
                    .module
                    .fixture_function_defs
                    .iter()
                    .map(|function| (function, false)),
            )
            .any(|(function, is_test)| {
                let target_parameters = function
                    .parameters
                    .iter_non_variadic_params()
                    .filter(|parameter| {
                        occurrences.iter().any(|occurrence| {
                            matches!(
                                occurrence.kind,
                                FixtureOccurrenceKind::Dependency
                                    | FixtureOccurrenceKind::TestParameter
                            ) && occurrence.range == parameter.parameter.name.range
                        })
                    })
                    .collect::<Vec<_>>();
                !target_parameters.is_empty()
                    && (function
                        .parameters
                        .iter_non_variadic_params()
                        .any(|parameter| {
                            parameter.parameter.name.as_str() == new_name
                                && target_parameters.iter().all(|target_parameter| {
                                    target_parameter.parameter.name.range
                                        != parameter.parameter.name.range
                                })
                        })
                        || is_test
                            && !crate::fixture::test_parameter_is_fixture(
                                &analysis.module,
                                function,
                                new_name,
                            ))
            })
}

#[cfg(test)]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use ruff_python_ast::PythonVersion;
    use ruff_text_size::TextSize;

    use super::*;
    use crate::occurrences::fixture_occurrence;
    use crate::{SourceAnalysisSettings, SourceDocument};

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
        assert!(rename_fixture(&index, &occurrence, "renamed").is_none());
    }

    #[test]
    fn returns_every_edit_for_a_valid_name() {
        let source = "from karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\ndef test_example(database): pass\n";
        let path = Utf8PathBuf::from("/project/test_example.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [SourceDocument::new(path.clone(), source.to_owned())],
            settings(),
        )
        .expect("source should index");
        let analysis = index.analyze(&path).expect("source should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(source, "database):"))
            .expect("parameter should resolve");

        let edits = rename_fixture(&index, &occurrence, "данные")
            .expect("valid Unicode identifier should rename");

        assert_eq!(edits.len(), 2);
        assert!(edits.iter().all(|edit| {
            let range = edit
                .occurrence
                .edit_range
                .expect("rename result should be editable");
            &source[range.to_std_range()] == "database"
        }));
    }

    #[test]
    fn rejects_invalid_python_identifiers() {
        assert!(!is_valid_fixture_name(""));
        assert!(!is_valid_fixture_name("class"));
        assert!(!is_valid_fixture_name("1database"));
        assert!(!is_valid_fixture_name("data-base"));
        assert!(!is_valid_fixture_name("data base"));
    }

    #[test]
    fn rejects_existing_fixture_and_builtin_names() {
        let source = "from karva import fixture\n@fixture\ndef database(): pass\n@fixture\ndef replacement(): pass\ndef test_example(database): pass\n";
        let path = Utf8PathBuf::from("/project/test_example.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [SourceDocument::new(path.clone(), source.to_owned())],
            settings(),
        )
        .expect("source should index");
        let analysis = index.analyze(&path).expect("source should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(source, "database):"))
            .expect("parameter should resolve");

        assert!(rename_fixture(&index, &occurrence, "replacement").is_none());
        assert!(rename_fixture(&index, &occurrence, "tmp_path").is_none());
    }

    #[test]
    fn rejects_parametrized_argument_name() {
        let source = "import pytest\nfrom karva import fixture\n@fixture\ndef database(): pass\n@pytest.mark.parametrize('replacement', [1])\ndef test_example(database, replacement): pass\n";
        let path = Utf8PathBuf::from("/project/test_example.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [SourceDocument::new(path.clone(), source.to_owned())],
            settings(),
        )
        .expect("source should index");
        let analysis = index.analyze(&path).expect("source should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(source, "database,"))
            .expect("parameter should resolve");

        assert!(rename_fixture(&index, &occurrence, "replacement").is_none());
    }

    #[test]
    fn rejects_dynamic_parametrization_for_the_visible_provider() {
        let source = "import pytest\nfrom karva import fixture\n@fixture\ndef database(): pass\n@pytest.mark.parametrize(names, [1])\ndef test_example(database): pass\n";
        let path = Utf8PathBuf::from("/project/test_example.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [SourceDocument::new(path.clone(), source.to_owned())],
            settings(),
        )
        .expect("source should index");
        let analysis = index.analyze(&path).expect("source should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(source, "database():"))
            .expect("definition should resolve");

        assert!(rename_fixture(&index, &occurrence, "replacement").is_none());
    }

    #[test]
    fn allows_an_unrelated_known_parametrized_name() {
        let source = "import pytest\nfrom karva import fixture\n@fixture\ndef database(): pass\n@pytest.mark.parametrize('database', [1])\ndef test_unrelated(database): pass\ndef test_fixture(database): pass\n";
        let path = Utf8PathBuf::from("/project/test_example.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [SourceDocument::new(path.clone(), source.to_owned())],
            settings(),
        )
        .expect("source should index");
        let analysis = index.analyze(&path).expect("source should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(source, "database():"))
            .expect("fixture definition should resolve");

        let edits = rename_fixture(&index, &occurrence, "replacement")
            .expect("known parametrization should be unrelated");
        assert_eq!(edits.len(), 2);
    }

    #[test]
    fn rejects_duplicate_test_parameters() {
        let source = "from karva import fixture\n@fixture\ndef database(): pass\ndef test_example(database, replacement): pass\n";
        let path = Utf8PathBuf::from("/project/test_example.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [SourceDocument::new(path.clone(), source.to_owned())],
            settings(),
        )
        .expect("source should index");
        let analysis = index.analyze(&path).expect("source should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(source, "database, replacement"))
            .expect("fixture parameter should resolve");

        assert!(rename_fixture(&index, &occurrence, "replacement").is_none());
    }

    #[test]
    fn rejects_duplicate_fixture_parameters() {
        let source = "from karva import fixture\n@fixture\ndef database(): pass\n@fixture\ndef wrapper(database, replacement): pass\n";
        let path = Utf8PathBuf::from("/project/test_example.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [SourceDocument::new(path.clone(), source.to_owned())],
            settings(),
        )
        .expect("source should index");
        let analysis = index.analyze(&path).expect("source should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(source, "database, replacement"))
            .expect("fixture dependency should resolve");

        assert!(rename_fixture(&index, &occurrence, "replacement").is_none());
    }
}
