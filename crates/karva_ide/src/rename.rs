use std::collections::HashSet;

use ruff_python_ast::visitor::source_order::{self, SourceOrderVisitor};
use ruff_python_ast::{Expr, ExprContext, Stmt};
use ruff_text_size::{Ranged, TextRange};

use crate::occurrences::{
    body_contains_name, body_contains_name_in_body, body_has_nested_binding_conflict,
    body_has_unsupported_bindings, expression_contains_name_in_scope, local_bindings,
};
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
/// rename instead of producing a partial workspace edit. Unsupported Python
/// scope declarations and class namespace bindings disable rename when their
/// references are not modeled by the source occurrence index.
pub fn prepare_fixture_rename(
    index: &WorkspaceSourceIndex,
    current: &FixtureOccurrence,
) -> Option<TextRange> {
    current.edit_range?;
    if has_unsafe_project_imports(index, &current.fixture) {
        return None;
    }
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
    if has_unsafe_project_imports(index, &current.fixture) {
        return None;
    }
    editable_fixture_occurrences(index, current, Some(new_name))
}

fn has_unsafe_project_imports(index: &WorkspaceSourceIndex, target: &FixtureId) -> bool {
    let Some((public_name, defining_name)) = index.paths().find_map(|path| {
        index
            .analyze(path)?
            .fixture_model
            .definition(target)
            .map(|definition| (definition.name.clone(), definition.defining_name.clone()))
    }) else {
        return false;
    };

    index.paths().any(|path| {
        let Some(analysis) = index.analyze(path) else {
            return true;
        };
        let known_imports = fixture_occurrences(&analysis)
            .into_iter()
            .filter(|occurrence| {
                occurrence.fixture == *target && occurrence.kind == FixtureOccurrenceKind::Import
            })
            .filter_map(|occurrence| occurrence.edit_range)
            .collect::<HashSet<_>>();
        analysis.module.module_body.iter().any(|statement| {
            let Stmt::ImportFrom(import) = statement else {
                return false;
            };
            import.names.iter().any(|alias| {
                let imported_name = alias.name.as_str();
                if imported_name == "*" {
                    return true;
                }
                let source_name_matches =
                    imported_name == public_name || imported_name == defining_name;
                let alias_name_matches = alias.asname.as_ref().is_some_and(|name| {
                    name.as_str() == public_name || name.as_str() == defining_name
                });
                (source_name_matches && !known_imports.contains(&alias.name.range()))
                    || (alias_name_matches
                        && alias
                            .asname
                            .as_ref()
                            .is_some_and(|name| !known_imports.contains(&name.range())))
            })
        })
    })
}

fn editable_fixture_occurrences(
    index: &WorkspaceSourceIndex,
    current: &FixtureOccurrence,
    new_name: Option<&str>,
) -> Option<Vec<LocatedFixtureOccurrence>> {
    let mut occurrences = Vec::new();
    for path in index.paths() {
        let analysis = index.analyze(path)?;
        let custom_name = analysis
            .fixture_model
            .definition(&current.fixture)
            .is_some_and(|definition| definition.name_range != definition.public_name_range);
        let preserved_imports = analysis
            .fixture_model
            .imports()
            .filter(|import| import.fixture == current.fixture && !import.rename_source)
            .map(|import| import.range)
            .collect::<HashSet<_>>();
        let matching = fixture_occurrences(&analysis)
            .into_iter()
            .filter(|occurrence| {
                occurrence.fixture == current.fixture
                    && !(custom_name && occurrence.kind == FixtureOccurrenceKind::Import)
                    && !(occurrence.kind == FixtureOccurrenceKind::Import
                        && preserved_imports.contains(&occurrence.range))
            })
            .collect::<Vec<_>>();
        if matching.is_empty() {
            continue;
        }
        if matching
            .iter()
            .any(|occurrence| occurrence.edit_range.is_none())
            || has_unsupported_parameter_bindings(&analysis, &matching)
            || has_unindexed_provider_references(&analysis, &current.fixture)
            || has_unsafe_import_scope(&analysis, &current.fixture, new_name)
            || new_name.is_some_and(|new_name| {
                fixture_name_conflicts(&analysis, &matching, &current.fixture, new_name)
                    || has_imported_binding_conflict(&analysis, &current.fixture, new_name)
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

fn has_unsafe_import_scope(
    analysis: &SourceAnalysis,
    target: &FixtureId,
    new_name: Option<&str>,
) -> bool {
    let old_names = analysis
        .fixture_model
        .imports()
        .filter(|import| import.fixture == *target && !import.has_alias && import.rename_source)
        .filter_map(|import| {
            analysis
                .fixture_model
                .definition(&import.fixture)
                .filter(|definition| definition.name == definition.defining_name)
                .map(|definition| definition.name.clone())
        })
        .collect::<HashSet<_>>();
    if old_names.is_empty() {
        return false;
    }

    let mut lambda_visitor = ImportedLambdaVisitor {
        old_names: &old_names,
        found: false,
    };
    source_order::walk_body(&mut lambda_visitor, &analysis.module.module_body);
    if lambda_visitor.found {
        return true;
    }

    analysis
        .module
        .module_body
        .iter()
        .any(|statement| match statement {
            Stmt::FunctionDef(function) => old_names.iter().any(|old_name| {
                if !body_contains_name(function, old_name) {
                    return false;
                }
                let bindings = local_bindings(function);
                bindings.contains(old_name)
                    || new_name.is_some_and(|new_name| bindings.contains(new_name))
                    || body_has_unsupported_bindings(function, &old_names)
                    || new_name.is_some_and(|new_name| {
                        body_has_nested_binding_conflict(function, old_name, new_name)
                    })
            }),
            Stmt::ClassDef(class) => old_names
                .iter()
                .any(|old_name| body_contains_name_in_body(&class.body, old_name)),
            _ => false,
        })
}

struct ImportedLambdaVisitor<'a> {
    old_names: &'a HashSet<String>,
    found: bool,
}

impl SourceOrderVisitor<'_> for ImportedLambdaVisitor<'_> {
    fn visit_expr(&mut self, expression: &'_ Expr) {
        if let Expr::Lambda(lambda) = expression {
            self.found |= self
                .old_names
                .iter()
                .any(|name| expression_contains_name_in_scope(&lambda.body, name, HashSet::new()));
        } else {
            source_order::walk_expr(self, expression);
        }
    }
}

fn has_imported_binding_conflict(
    analysis: &SourceAnalysis,
    target: &FixtureId,
    new_name: &str,
) -> bool {
    let has_unaliased_import = analysis.fixture_model.imports().any(|import| {
        if import.fixture != *target || import.has_alias || !import.rename_source {
            return false;
        }
        analysis
            .fixture_model
            .definition(&import.fixture)
            .is_some_and(|definition| definition.name == definition.defining_name)
    });
    if !has_unaliased_import {
        return false;
    }

    let mut visitor = ImportedBindingConflictVisitor {
        name: new_name,
        found: false,
    };
    source_order::walk_body(&mut visitor, &analysis.module.module_body);
    visitor.found
}

struct ImportedBindingConflictVisitor<'a> {
    name: &'a str,
    found: bool,
}

impl SourceOrderVisitor<'_> for ImportedBindingConflictVisitor<'_> {
    fn visit_expr(&mut self, expression: &'_ Expr) {
        if let Expr::Name(name) = expression
            && name.id == self.name
        {
            self.found = true;
        } else {
            source_order::walk_expr(self, expression);
        }
    }

    fn visit_alias(&mut self, alias: &'_ ruff_python_ast::Alias) {
        self.found |= alias.name.as_str() == self.name
            || alias
                .asname
                .as_ref()
                .is_some_and(|name| name.as_str() == self.name);
    }
}

fn has_unindexed_provider_references(analysis: &SourceAnalysis, target: &FixtureId) -> bool {
    let Some(definition) = analysis.fixture_model.definition(target) else {
        return false;
    };
    let occurrences = fixture_occurrences(analysis)
        .into_iter()
        .filter(|occurrence| occurrence.fixture == *target)
        .collect::<Vec<_>>();
    let known_ranges = occurrences
        .iter()
        .flat_map(|occurrence| std::iter::once(occurrence.range).chain(occurrence.edit_range))
        .collect::<HashSet<_>>();
    let known_import_ranges = occurrences
        .iter()
        .filter(|occurrence| occurrence.kind == FixtureOccurrenceKind::Import)
        .map(|occurrence| occurrence.range)
        .collect::<HashSet<_>>();
    let mut visitor = ProviderReferenceVisitor {
        name: &definition.name,
        known_ranges: &known_ranges,
        known_import_ranges: &known_import_ranges,
        found: false,
    };
    source_order::walk_body(&mut visitor, &analysis.module.module_body);
    visitor.found
}

struct ProviderReferenceVisitor<'a> {
    name: &'a str,
    known_ranges: &'a HashSet<ruff_text_size::TextRange>,
    known_import_ranges: &'a HashSet<ruff_text_size::TextRange>,
    found: bool,
}

impl SourceOrderVisitor<'_> for ProviderReferenceVisitor<'_> {
    fn visit_expr(&mut self, expression: &'_ Expr) {
        if let Expr::Name(name) = expression
            && name.id == self.name
            && if matches!(name.ctx, ExprContext::Store | ExprContext::Del) {
                !self.known_import_ranges.contains(&name.range)
            } else {
                !self.known_ranges.contains(&name.range)
            }
        {
            self.found = true;
        } else {
            source_order::walk_expr(self, expression);
        }
    }

    fn visit_alias(&mut self, alias: &'_ ruff_python_ast::Alias) {
        let imported_name = alias.name.as_str().rsplit('.').next().unwrap_or_default();
        if imported_name == "*"
            || (imported_name == self.name && !self.known_ranges.contains(&alias.name.range()))
        {
            self.found = true;
        } else if let Some(asname) = &alias.asname
            && asname.as_str() == self.name
            && !self.known_ranges.contains(&asname.range())
        {
            self.found = true;
        }
    }
}

fn has_unsupported_parameter_bindings(
    analysis: &SourceAnalysis,
    occurrences: &[FixtureOccurrence],
) -> bool {
    analysis
        .module
        .test_function_defs
        .iter()
        .chain(&analysis.module.fixture_function_defs)
        .any(|function| {
            let target_names = function
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
                .map(|parameter| parameter.parameter.name.as_str().to_owned())
                .collect::<std::collections::HashSet<_>>();
            !target_names.is_empty() && body_has_unsupported_bindings(function, &target_names)
        })
}

fn fixture_name_conflicts(
    analysis: &SourceAnalysis,
    occurrences: &[FixtureOccurrence],
    target: &FixtureId,
    new_name: &str,
) -> bool {
    let target_name = analysis
        .fixture_model
        .definition(target)
        .map(|definition| definition.name.as_str());
    if target_name == Some(new_name) {
        return false;
    }

    analysis.fixture_model.blocked_names().contains(new_name)
        || analysis
            .fixture_model
            .visible()
            .iter()
            .any(|definition| definition.name == new_name && &definition.id != target)
        || crate::fixture::builtin_info(new_name).is_some()
        || target_name.is_some_and(|target_name| {
            analysis
                .fixture_model
                .visible()
                .iter()
                .any(|definition| &definition.id == target && definition.name == target_name)
                && analysis.module.test_function_defs.iter().any(|function| {
                    analysis.fixture_model.parametrization_is_dynamic(function)
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
                        || body_contains_name(function, new_name)
                        || target_parameters.iter().any(|parameter| {
                            body_has_nested_binding_conflict(
                                function,
                                parameter.parameter.name.as_str(),
                                new_name,
                            )
                        })
                        || is_test
                            && !analysis
                                .fixture_model
                                .parameter_is_fixture(function, new_name))
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
    fn renames_fixture_body_references() {
        let source = "from karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\ndef test_example(database):\n    return database\n";
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

        let edits = rename_fixture(&index, &occurrence, "renamed_database")
            .expect("body reference should be renamed");

        assert_eq!(edits.len(), 3);
        assert_eq!(
            edits
                .iter()
                .map(|edit| &source[edit.occurrence.range.to_std_range()])
                .collect::<Vec<_>>(),
            ["database", "database", "database"]
        );
    }

    #[test]
    fn rejects_rename_when_default_provider_has_ordinary_uses() {
        let source = "from karva import fixture\n@fixture\ndef database(): pass\ndef helper():\n    return database()\n";
        let path = Utf8PathBuf::from("/project/conftest.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [SourceDocument::new(path.clone(), source.to_owned())],
            settings(),
        )
        .expect("source should index");
        let analysis = index.analyze(&path).expect("source should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(source, "database():"))
            .expect("provider should resolve");

        assert!(rename_fixture(&index, &occurrence, "renamed_database").is_none());
    }

    #[test]
    fn custom_provider_name_keeps_ordinary_provider_uses() {
        let source = "from karva import fixture\n@fixture(name=\"database\")\ndef provider():\n    return provider()\n";
        let path = Utf8PathBuf::from("/project/conftest.py");
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [SourceDocument::new(path.clone(), source.to_owned())],
            settings(),
        )
        .expect("source should index");
        let analysis = index.analyze(&path).expect("source should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(source, "database\""))
            .expect("public name should resolve");

        assert!(rename_fixture(&index, &occurrence, "renamed_database").is_some());
    }

    #[test]
    fn renames_default_fixture_import_source_but_preserves_alias() {
        let provider = "from karva import fixture\n@fixture\ndef database(): pass\n";
        let reexport = "from support import database as db\n";
        let test = "def test_example(database): pass\n";
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [
                SourceDocument::new("/project/support.py".into(), provider.to_owned()),
                SourceDocument::new("/project/conftest.py".into(), reexport.to_owned()),
                SourceDocument::new("/project/test_example.py".into(), test.to_owned()),
            ],
            settings(),
        )
        .expect("sources should index");
        let analysis = index
            .analyze(Utf8Path::new("/project/test_example.py"))
            .expect("test should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(test, "database")).expect("target");

        let edits = rename_fixture(&index, &occurrence, "renamed_database")
            .expect("default fixture should rename through imports");
        assert_eq!(edits.len(), 3);
        let import = edits
            .iter()
            .find(|edit| edit.path == Utf8Path::new("/project/conftest.py"))
            .expect("reexport should be edited");
        assert_eq!(
            &reexport[import
                .occurrence
                .edit_range
                .expect("import should edit")
                .to_std_range()],
            "database"
        );
        assert_eq!(&reexport[import.occurrence.range.to_std_range()], "db");
    }

    #[test]
    fn custom_fixture_import_does_not_disable_public_name_rename() {
        let provider =
            "from karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\n";
        let reexport = "from support import provider as db\n";
        let test = "def test_example(database): pass\n";
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [
                SourceDocument::new("/project/support.py".into(), provider.to_owned()),
                SourceDocument::new("/project/conftest.py".into(), reexport.to_owned()),
                SourceDocument::new("/project/test_example.py".into(), test.to_owned()),
            ],
            settings(),
        )
        .expect("sources should index");
        let analysis = index
            .analyze(Utf8Path::new("/project/test_example.py"))
            .expect("test should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(test, "database")).expect("target");

        let edits = rename_fixture(&index, &occurrence, "renamed_database")
            .expect("custom fixture should rename through imports");
        assert_eq!(edits.len(), 2);
        assert!(
            edits
                .iter()
                .all(|edit| { edit.path != Utf8Path::new("/project/conftest.py") })
        );
    }

    #[test]
    fn preserves_aliases_across_reexport_chains() {
        let provider = "from karva import fixture\n@fixture\ndef database(): pass\n";
        let first = "from support import database as db\n";
        let second = "from conftest import db\n";
        let test = "def test_example(database): pass\n";
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [
                SourceDocument::new("/project/support.py".into(), provider.to_owned()),
                SourceDocument::new("/project/conftest.py".into(), first.to_owned()),
                SourceDocument::new("/project/pkg/conftest.py".into(), second.to_owned()),
                SourceDocument::new("/project/pkg/test_example.py".into(), test.to_owned()),
            ],
            settings(),
        )
        .expect("sources should index");
        let analysis = index
            .analyze(Utf8Path::new("/project/pkg/test_example.py"))
            .expect("test should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(test, "database")).expect("target");

        let edits = rename_fixture(&index, &occurrence, "renamed_database")
            .expect("fixture should rename through reexports");
        assert_eq!(edits.len(), 3);
        assert!(
            edits
                .iter()
                .all(|edit| { edit.path != Utf8Path::new("/project/pkg/conftest.py") })
        );
    }

    #[test]
    fn renames_unaliased_import_binding_uses() {
        let provider = "from karva import fixture\n@fixture\ndef database(): pass\n";
        let reexport = "from support import database\nvalue = database()\n";
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [
                SourceDocument::new("/project/support.py".into(), provider.to_owned()),
                SourceDocument::new("/project/conftest.py".into(), reexport.to_owned()),
            ],
            settings(),
        )
        .expect("sources should index");
        let analysis = index
            .analyze(Utf8Path::new("/project/support.py"))
            .expect("provider should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(provider, "database():"))
            .expect("provider should resolve");

        let edits = rename_fixture(&index, &occurrence, "renamed_database")
            .expect("fixture should rename binding uses");
        assert_eq!(edits.len(), 3);
        assert_eq!(
            edits
                .iter()
                .filter(|edit| edit.path == Utf8Path::new("/project/conftest.py"))
                .count(),
            2
        );
    }

    #[test]
    fn rejects_import_rename_with_unrelated_local_binding() {
        let provider = "from karva import fixture\n@fixture\ndef database(): pass\n";
        let reexport = "from support import database\ndef helper(database):\n    return database\n";
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [
                SourceDocument::new("/project/support.py".into(), provider.to_owned()),
                SourceDocument::new("/project/conftest.py".into(), reexport.to_owned()),
            ],
            settings(),
        )
        .expect("sources should index");
        let analysis = index
            .analyze(Utf8Path::new("/project/support.py"))
            .expect("provider should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(provider, "database():"))
            .expect("provider should resolve");

        assert!(rename_fixture(&index, &occurrence, "renamed_database").is_none());
    }

    #[test]
    fn rejects_wildcard_fixture_reexports() {
        let provider = "from karva import fixture\n@fixture\ndef database(): pass\n";
        let reexport = "from support import *\n";
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [
                SourceDocument::new("/project/support.py".into(), provider.to_owned()),
                SourceDocument::new("/project/conftest.py".into(), reexport.to_owned()),
            ],
            settings(),
        )
        .expect("sources should index");
        let analysis = index
            .analyze(Utf8Path::new("/project/support.py"))
            .expect("provider should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(provider, "database():"))
            .expect("provider should resolve");

        assert!(rename_fixture(&index, &occurrence, "renamed_database").is_none());
    }

    #[test]
    fn rejects_unresolved_fixture_name_imports() {
        let provider = "from karva import fixture\n@fixture\ndef database(): pass\n";
        let unresolved = "from missing import database\n";
        let index = WorkspaceSourceIndex::from_documents(
            "/project".into(),
            [
                SourceDocument::new("/project/support.py".into(), provider.to_owned()),
                SourceDocument::new("/project/conftest.py".into(), unresolved.to_owned()),
            ],
            settings(),
        )
        .expect("sources should index");
        let analysis = index
            .analyze(Utf8Path::new("/project/support.py"))
            .expect("provider should analyze");
        let occurrence = fixture_occurrence(&analysis, offset(provider, "database():"))
            .expect("provider should resolve");

        assert!(rename_fixture(&index, &occurrence, "renamed_database").is_none());
    }

    #[test]
    fn rejects_rename_that_would_capture_a_body_name() {
        let source = "from karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\ndef test_example(database):\n    return replacement\n";
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
    }

    #[test]
    fn rejects_rename_that_would_capture_a_nested_parameter() {
        let source = "from karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\ndef test_example(database):\n    def inner(replacement):\n        return database\n    return inner\n";
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
    }

    #[test]
    fn rejects_rename_with_unsupported_scope_bindings() {
        let source = "from karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\ndef test_example(database):\n    global database\n    return database\n";
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

        assert!(rename_fixture(&index, &occurrence, "renamed_database").is_none());
    }

    #[test]
    fn rejects_rename_with_class_namespace_bindings() {
        let source = "from karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\ndef test_example(database):\n    class Inner:\n        database = object()\n        value = database\n    return database\n";
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

        assert!(rename_fixture(&index, &occurrence, "renamed_database").is_none());
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
