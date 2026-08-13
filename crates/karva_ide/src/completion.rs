use std::convert::TryFrom;

use ruff_python_ast::Expr;
use ruff_text_size::{Ranged, TextRange, TextSize};

use crate::{FixtureId, FixtureScope, SourceAnalysis};

/// A fixture completion independent of an editor protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureCompletion {
    /// Text inserted by the editor.
    pub label: String,

    /// Human-readable provider, scope, and autouse information.
    pub detail: String,

    /// Range replaced by `label`.
    pub replacement_range: TextRange,
}

/// Computes fixture completions at a UTF-8 byte offset.
///
/// Dynamic expressions and positions outside test/fixture parameters or known
/// `use_fixtures` decorators return `None`.
pub fn complete_fixtures(
    analysis: &SourceAnalysis,
    offset: TextSize,
) -> Option<Vec<FixtureCompletion>> {
    let source = &analysis.module.source_text;
    let replacement =
        parameter_context(analysis, offset).or_else(|| use_fixtures_context(analysis, offset))?;
    let prefix = source.get(replacement.start().to_usize()..replacement.end().to_usize())?;

    let mut completions = Vec::new();
    let mut names = analysis.fixture_completion_blocked_names.clone();
    for fixture in &analysis.visible_fixtures {
        if names.insert(fixture.name.clone()) && fixture.name.starts_with(prefix) {
            completions.push(completion(
                fixture.name.clone(),
                replacement,
                Some(&fixture.id),
                fixture.scope,
                fixture.auto_use,
            ));
        }
    }
    if analysis.fixture_completion_builtins_visible {
        for (name, scope) in crate::fixture::builtin_fixtures() {
            if names.insert(name.to_owned()) && name.starts_with(prefix) {
                completions.push(completion(
                    name.to_owned(),
                    replacement,
                    None,
                    Some(scope),
                    Some(false),
                ));
            }
        }
    }
    completions.sort_by(|left, right| left.label.cmp(&right.label));
    Some(completions)
}

fn completion(
    label: String,
    replacement_range: TextRange,
    provider: Option<&FixtureId>,
    scope: Option<FixtureScope>,
    auto_use: Option<bool>,
) -> FixtureCompletion {
    let origin = provider.map_or_else(|| "Karva built-in".to_owned(), |id| id.path.to_string());
    let scope_detail =
        scope.map_or_else(|| "dynamic".to_owned(), |scope| scope.as_str().to_owned());
    let auto_use_detail =
        auto_use.map_or_else(|| "dynamic".to_owned(), |auto_use| auto_use.to_string());
    FixtureCompletion {
        label,
        detail: format!("fixture · {origin} · scope={scope_detail} · autouse={auto_use_detail}"),
        replacement_range,
    }
}

fn parameter_context(analysis: &SourceAnalysis, offset: TextSize) -> Option<TextRange> {
    let source = &analysis.module.source_text;
    let functions = analysis
        .module
        .test_function_defs
        .iter()
        .chain(&analysis.module.fixture_function_defs);
    for function in functions {
        let parameters = function.parameters.range;
        if !parameters.contains_inclusive(offset) {
            continue;
        }
        if let Some(parameter) = function
            .parameters
            .iter_non_variadic_params()
            .find(|parameter| parameter.parameter.name.range.contains_inclusive(offset))
        {
            return Some(parameter.parameter.name.range);
        }
        let token = identifier_range(source, offset, parameters);
        if let Some(token) = token {
            if parameter_slot(source, parameters, token.start()) {
                return Some(token);
            }
        } else if parameter_slot(source, parameters, offset) {
            return Some(TextRange::at(offset, TextSize::ZERO));
        }
    }
    None
}

fn parameter_slot(source: &str, parameters: TextRange, offset: TextSize) -> bool {
    let start = parameters.start().to_usize();
    let end = offset.to_usize();
    let Some(prefix) = source.get(start..end) else {
        return false;
    };
    let segment = prefix
        .rsplit_once(',')
        .map_or(prefix, |(_, segment)| segment);
    let segment = segment.trim();
    !segment.is_empty()
        && segment
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
        || segment.is_empty()
}

fn use_fixtures_context(analysis: &SourceAnalysis, offset: TextSize) -> Option<TextRange> {
    let source = &analysis.module.source_text;
    for function in &analysis.module.test_function_defs {
        for decorator in &function.decorator_list {
            if !decorator.range().contains_inclusive(offset) {
                continue;
            }
            let Expr::Call(call) = &decorator.expression else {
                continue;
            };
            if !crate::fixture::is_use_fixtures_reference(&analysis.module, &call.func) {
                continue;
            }
            for argument in &call.arguments.args {
                let Expr::StringLiteral(literal) = argument else {
                    return None;
                };
                let interior = crate::fixture::single_string_content_range(literal)?;
                if interior.contains_inclusive(offset) {
                    return Some(
                        identifier_range(source, offset, interior)
                            .unwrap_or_else(|| TextRange::at(offset, TextSize::ZERO)),
                    );
                }
            }
        }
    }
    None
}

fn identifier_range(source: &str, offset: TextSize, boundary: TextRange) -> Option<TextRange> {
    let offset = offset.to_usize();
    let start = boundary.start().to_usize();
    let end = boundary.end().to_usize().min(source.len());
    if offset < start || offset > end {
        return None;
    }
    if !source.is_char_boundary(offset) {
        return None;
    }
    let mut token_start = offset;
    for (index, character) in source.get(start..offset)?.char_indices().rev() {
        if !is_identifier_character(character) {
            break;
        }
        token_start = start + index;
    }
    let mut token_end = offset;
    for (index, character) in source.get(offset..end)?.char_indices() {
        if !is_identifier_character(character) {
            break;
        }
        token_end = offset + index + character.len_utf8();
    }
    if token_start >= token_end {
        return None;
    }
    Some(TextRange::new(size(token_start)?, size(token_end)?))
}

fn is_identifier_character(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

fn size(value: usize) -> Option<TextSize> {
    u32::try_from(value).ok().map(TextSize::from)
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

    fn settings() -> SourceAnalysisSettings {
        SourceAnalysisSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test".to_owned(),
            try_import_fixtures: false,
        }
    }

    fn analysis(source: &str) -> SourceAnalysis {
        analyze_source(
            &Utf8PathBuf::from("/project/test_example.py"),
            Utf8Path::new("/project"),
            source.to_owned(),
            &settings(),
        )
        .expect("source should analyze")
    }

    fn at(source: &str, marker: &str) -> TextSize {
        size(source.find(marker).expect("marker")).expect("source fits in TextSize")
    }

    #[test]
    fn completes_local_and_builtin_fixtures_in_test_parameters() {
        let source = "from karva import fixture\n\n@fixture\ndef database(): pass\n\ndef test_example(dat): pass\n";
        let analysis = analysis(source);
        let marker = source.rfind("dat").expect("test parameter");
        let completions = complete_fixtures(
            &analysis,
            size(marker).expect("source fits") + TextSize::from(2),
        )
        .expect("parameter completion");
        assert_eq!(completions[0].label, "database");
        assert!(
            completions
                .iter()
                .any(|completion| completion.label == "database")
        );
        assert_eq!(
            completions[0].replacement_range,
            TextRange::new(
                size(marker).expect("source fits"),
                size(marker).expect("source fits") + TextSize::from(3),
            )
        );
    }

    #[test]
    fn parent_override_and_rejected_fixtures_follow_runtime_visibility() {
        let parent = SourceDocument::new(
            Utf8PathBuf::from("/project/conftest.py"),
            "from karva import fixture\n\n@fixture\ndef database(): pass\n@fixture\ndef parent_only(): pass\n".to_owned(),
        );
        let source = "from karva import fixture\n\n@fixture(scope=\"invalid\")\ndef database(): pass\n\ndef test_example(pa): pass\n";
        let current = SourceDocument::new(
            Utf8PathBuf::from("/project/test_example.py"),
            source.to_owned(),
        );
        let analysis =
            analyze_source_with_parents(current, [parent], Utf8Path::new("/project"), &settings())
                .expect("source should analyze");
        let marker = source.find("pa):").expect("test parameter");
        let completions = complete_fixtures(
            &analysis,
            size(marker).expect("source fits") + TextSize::from(2),
        )
        .expect("completion");
        assert!(
            completions
                .iter()
                .all(|completion| completion.label != "database")
        );
        assert!(
            completions
                .iter()
                .any(|completion| completion.label == "parent_only")
        );
    }

    #[test]
    fn rejected_fixture_name_blocks_builtin_completion() {
        let source = "from karva import fixture\n\n@fixture(scope=\"invalid\")\ndef tmp_path(): pass\n\ndef test_example(tmp): pass\n";
        let analysis = analysis(source);
        let marker = source.rfind("tmp").expect("test parameter");
        let completions = complete_fixtures(
            &analysis,
            size(marker).expect("source fits") + TextSize::from(3),
        )
        .expect("parameter completion");

        assert!(
            completions
                .iter()
                .all(|completion| completion.label != "tmp_path")
        );
    }

    #[test]
    fn replaces_unicode_parameter_prefix() {
        let source = "from karva import fixture\n\n@fixture\ndef δεδομενα(): pass\n\ndef test_example(δε): pass\n";
        let analysis = analysis(source);
        let marker = source.rfind("δε").expect("test parameter");
        let completions =
            complete_fixtures(&analysis, size(marker + "δε".len()).expect("source fits"))
                .expect("parameter completion");

        assert_eq!(completions[0].label, "δεδομενα");
        assert_eq!(
            completions[0].replacement_range,
            TextRange::new(
                size(marker).expect("source fits"),
                size(marker + "δε".len()).expect("source fits"),
            )
        );
    }

    #[test]
    fn completes_fixture_parameters_and_use_fixtures_strings() {
        let source = "from karva import fixture\nimport karva\n\n@fixture\ndef database(): pass\n\n@karva.tags.use_fixtures(\"dat\")\ndef test_example(): pass\n\n@fixture\ndef wrapper(dat): pass\n";
        let analysis = analysis(source);
        let first = source.find("\"dat\"").expect("use_fixtures argument") + 1;
        assert!(
            complete_fixtures(
                &analysis,
                size(first).expect("source fits") + TextSize::from(2)
            )
            .is_some()
        );
        let second = source.rfind("dat").expect("fixture parameter");
        assert!(
            complete_fixtures(
                &analysis,
                size(second).expect("source fits") + TextSize::from(2)
            )
            .is_some()
        );
    }

    #[test]
    fn custom_test_prefix_is_collected() {
        let source = "def spec_example(dat): pass\n";
        let settings = SourceAnalysisSettings {
            test_function_prefix: "spec".to_owned(),
            ..settings()
        };
        let analysis = analyze_source(
            &Utf8PathBuf::from("/project/test_example.py"),
            Utf8Path::new("/project"),
            source.to_owned(),
            &settings,
        )
        .expect("source should analyze");
        let marker = source.find("dat").expect("fixture parameter");
        assert!(
            complete_fixtures(
                &analysis,
                size(marker).expect("source fits") + TextSize::from(2)
            )
            .is_some()
        );
    }

    #[test]
    fn dynamic_context_is_not_completed() {
        let source = "def helper(dat): pass\n";
        assert!(
            complete_fixtures(&analysis(source), at(source, "dat") + TextSize::from(2)).is_none()
        );
    }

    #[test]
    fn shadowed_pytest_name_is_not_a_usefixtures_context() {
        let source = "import other as pytest\nfrom karva import fixture\n\n@fixture\ndef database(): pass\n\n@pytest.mark.usefixtures(\"dat\")\ndef test_example(): pass\n";
        let marker = source.find("\"dat\"").expect("decorator argument") + 1;

        assert!(
            complete_fixtures(
                &analysis(source),
                size(marker).expect("source fits") + TextSize::from(2)
            )
            .is_none()
        );
    }
}
