use ruff_python_ast::{Expr, StmtFunctionDef};
use ruff_text_size::{Ranged, TextRange, TextSize};

use crate::{FixtureDefinition, FixtureId, FixtureResolution, FixtureScope, SourceAnalysis};

/// Source-only hover information for one fixture reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureHover {
    /// Fixture name under the cursor.
    pub name: String,

    /// UTF-8 byte range of the reference.
    pub range: TextRange,

    /// Source provider, or `None` for a built-in fixture.
    pub provider: Option<FixtureId>,

    /// Statically known fixture scope.
    pub scope: Option<FixtureScope>,

    /// Statically known autouse setting.
    pub auto_use: Option<bool>,

    /// Provider function signature, or built-in signature.
    pub source_signature: String,

    /// Provider docstring, when statically available.
    pub docstring: Option<String>,

    /// Direct fixture dependencies by public name.
    pub dependencies: Vec<String>,
}

/// Resolves a fixture reference at a UTF-8 byte offset.
///
/// Only statically resolved references produce hover information. Rejected,
/// missing, and dynamic providers remain silent rather than showing a target
/// that runtime lookup cannot guarantee.
pub fn hover_fixture(analysis: &SourceAnalysis, offset: TextSize) -> Option<FixtureHover> {
    if let Some(reference) = analysis.fixtures.iter().find_map(|fixture| {
        fixture
            .dependencies
            .iter()
            .find(|reference| reference.range.contains_inclusive(offset))
            .cloned()
    }) {
        return hover_from_resolution(
            analysis,
            reference.name.clone(),
            reference.range,
            &reference.resolution,
        );
    }

    for function in analysis
        .module
        .test_function_defs
        .iter()
        .chain(&analysis.module.fixture_function_defs)
    {
        if let Some(reference) = test_parameter_reference(analysis, function, offset) {
            return hover_from_name(analysis, reference.0, reference.1);
        }
    }

    for function in &analysis.module.test_function_defs {
        if let Some((name, range)) = use_fixtures_reference(analysis, function, offset) {
            return hover_from_name(analysis, name, range);
        }
    }

    None
}

fn hover_from_name(
    analysis: &SourceAnalysis,
    name: String,
    range: TextRange,
) -> Option<FixtureHover> {
    let definition = analysis
        .visible_fixtures
        .iter()
        .find(|fixture| fixture.name == name);
    if analysis.fixture_completion_blocked_names.contains(&name) {
        return None;
    }
    if let Some(definition) = definition {
        return Some(hover_from_definition(name, range, definition));
    }
    if !analysis.fixture_completion_builtins_visible {
        return None;
    }
    let builtin = crate::fixture::builtin_info(&name)?;
    Some(FixtureHover {
        name,
        range,
        provider: None,
        scope: Some(builtin.scope),
        auto_use: Some(false),
        source_signature: builtin.signature.to_owned(),
        docstring: Some(builtin.docstring.to_owned()),
        dependencies: Vec::new(),
    })
}

fn hover_from_resolution(
    analysis: &SourceAnalysis,
    name: String,
    range: TextRange,
    resolution: &FixtureResolution,
) -> Option<FixtureHover> {
    match resolution {
        FixtureResolution::Resolved(id) => {
            let definition = analysis
                .visible_fixtures
                .iter()
                .find(|fixture| fixture.id == *id)?;
            Some(hover_from_definition(name, range, definition))
        }
        FixtureResolution::Builtin => hover_from_name(analysis, name, range),
        FixtureResolution::Rejected(_)
        | FixtureResolution::Missing
        | FixtureResolution::Unknown => None,
    }
}

fn hover_from_definition(
    name: String,
    range: TextRange,
    definition: &FixtureDefinition,
) -> FixtureHover {
    FixtureHover {
        name,
        range,
        provider: Some(definition.id.clone()),
        scope: definition.scope,
        auto_use: definition.auto_use,
        source_signature: definition.signature.clone(),
        docstring: definition.docstring.clone(),
        dependencies: definition
            .dependencies
            .iter()
            .map(|dependency| dependency.name.clone())
            .collect(),
    }
}

fn test_parameter_reference(
    analysis: &SourceAnalysis,
    function: &StmtFunctionDef,
    offset: TextSize,
) -> Option<(String, TextRange)> {
    let parameter = function
        .parameters
        .iter_non_variadic_params()
        .find(|parameter| parameter.parameter.name.range.contains_inclusive(offset))?;
    if !crate::fixture::test_parameter_is_fixture(
        &analysis.module,
        function,
        parameter.parameter.name.as_str(),
    ) {
        return None;
    }
    let name = parameter.parameter.name.to_string();
    if analysis.fixture_completion_blocked_names.contains(&name) {
        return None;
    }
    Some((name, parameter.parameter.name.range))
}

fn use_fixtures_reference(
    analysis: &SourceAnalysis,
    function: &StmtFunctionDef,
    offset: TextSize,
) -> Option<(String, TextRange)> {
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
            if !interior.contains_inclusive(offset) {
                continue;
            }
            return Some((literal.value.to_str().to_owned(), interior));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use ruff_python_ast::PythonVersion;

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

    fn at(source: &str, marker: &str) -> TextSize {
        TextSize::try_from(source.find(marker).expect("marker exists")).expect("source fits")
    }

    #[test]
    fn hovers_local_fixture_with_signature_docstring_and_dependencies() {
        let source = "from karva import fixture\n\n@fixture(scope=\"module\")\ndef database():\n    \"\"\"Open test database.\"\"\"\n\n@fixture\ndef wrapper(database): pass\n\ndef test_example(wrapper): pass\n";
        let result = hover_fixture(
            &analysis(source),
            at(source, "wrapper):") + TextSize::from(2),
        )
        .expect("local fixture hover");
        assert_eq!(result.name, "wrapper");
        assert_eq!(result.scope, Some(FixtureScope::Function));
        assert_eq!(result.auto_use, Some(false));
        assert_eq!(result.dependencies, ["database"]);
        assert_eq!(result.docstring, None);
        assert!(
            result
                .source_signature
                .starts_with("def wrapper(database):")
        );

        let dependency = hover_fixture(
            &analysis(source),
            at(source, "database):") + TextSize::from(4),
        )
        .expect("dependency hover");
        assert_eq!(dependency.name, "database");
        assert_eq!(dependency.docstring.as_deref(), Some("Open test database."));
    }

    #[test]
    fn hovers_parent_override_provider() {
        let parent = SourceDocument::new(
            Utf8PathBuf::from("/project/conftest.py"),
            "from karva import fixture\n@fixture\ndef database(): pass\n".to_owned(),
        );
        let source = "from karva import fixture\n@fixture\ndef database(): pass\n\ndef test_example(database): pass\n";
        let current = SourceDocument::new(
            Utf8PathBuf::from("/project/test_example.py"),
            source.to_owned(),
        );
        let result = hover_fixture(
            &analyze_source_with_parents(current, [parent], Utf8Path::new("/project"), &settings())
                .expect("source should analyze"),
            at(source, "database):") + TextSize::from(2),
        )
        .expect("override hover");
        assert_eq!(
            result.provider.expect("provider").path,
            "/project/test_example.py"
        );
    }

    #[test]
    fn hovers_builtin_and_suppresses_unknown_fixture() {
        let source = "def test_example(tmp_path): pass\n\ndef test_unknown(missing): pass\n";
        let result = hover_fixture(
            &analysis(source),
            at(source, "tmp_path):") + TextSize::from(2),
        )
        .expect("builtin hover");
        assert_eq!(result.name, "tmp_path");
        assert_eq!(result.provider, None);
        assert!(
            hover_fixture(
                &analysis(source),
                at(source, "missing):") + TextSize::from(4)
            )
            .is_none()
        );
    }

    #[test]
    fn hovers_use_fixtures_string() {
        let source = "import pytest\nfrom karva import fixture\n@fixture\ndef database(): pass\n@pytest.mark.usefixtures(\"database\")\ndef test_example(): pass\n";
        let result = hover_fixture(
            &analysis(source),
            at(source, "database\"") + TextSize::from(2),
        )
        .expect("usefixtures hover");
        assert_eq!(result.name, "database");
        assert_eq!(
            result.range,
            TextRange::new(
                at(source, "database\""),
                at(source, "database\"") + TextSize::from(8)
            )
        );
    }

    #[test]
    fn hovers_escaped_use_fixtures_string() {
        let source = "import pytest\nfrom karva import fixture\n@fixture\ndef database(): pass\n@pytest.mark.usefixtures(\"data\\x62ase\")\ndef test_example(): pass\n";
        let result =
            hover_fixture(&analysis(source), at(source, "x62")).expect("escaped usefixtures hover");

        assert_eq!(result.name, "database");
        assert_eq!(
            result.range,
            TextRange::new(
                at(source, "data\\x62ase"),
                at(source, "data\\x62ase") + TextSize::from(11),
            )
        );
    }

    #[test]
    fn hovers_unicode_fixture_name_by_utf8_offset() {
        let source = "from karva import fixture\n@fixture\ndef δεδομενα(): pass\ndef test_example(δεδομενα): pass\n";
        let offset =
            at(source, "δεδομενα):") + TextSize::try_from("δεδομενα".len()).expect("source fits");
        let result = hover_fixture(&analysis(source), offset).expect("unicode hover");
        assert_eq!(result.name, "δεδομενα");
    }

    #[test]
    fn shadowed_pytest_name_does_not_create_framework_hover() {
        let source = "import other as pytest\nfrom karva import fixture\n@fixture\ndef database(): pass\n@pytest.mark.usefixtures(\"database\")\ndef test_example(): pass\n";
        let offset = at(source, "\"database\"") + TextSize::from(3);

        assert!(hover_fixture(&analysis(source), offset).is_none());
    }
}
