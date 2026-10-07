#![expect(
    clippy::redundant_pub_crate,
    reason = "references and rename consume occurrence APIs across private sibling modules"
)]

use std::collections::{HashMap, HashSet};

use ruff_python_ast::visitor::source_order::{self, SourceOrderVisitor};
use ruff_python_ast::{Comprehension, Expr, ExprContext, Stmt, StmtFunctionDef};
use ruff_text_size::{Ranged, TextRange, TextSize};

use crate::{FixtureId, FixtureResolution, SourceAnalysis};

/// The source construct containing a fixture occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureOccurrenceKind {
    /// A fixture provider declaration.
    Definition,

    /// A fixture dependency parameter.
    Dependency,

    /// A test function parameter supplied by a fixture.
    TestParameter,

    /// A fixture name in `usefixtures` metadata.
    UseFixtures,

    /// A name in a fixture-consuming function body.
    BodyReference,
}

/// A source occurrence resolved to one fixture provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureOccurrence {
    /// The occurrence's source range.
    pub range: TextRange,

    /// Range that can be replaced with another fixture name.
    ///
    /// This is `None` when implicit string concatenation prevents a safe
    /// single edit.
    pub edit_range: Option<TextRange>,

    /// The kind of source construct containing the occurrence.
    pub kind: FixtureOccurrenceKind,

    /// The fixture provider selected by static analysis.
    pub(super) fixture: FixtureId,
}

/// Fixture selected for rename plus its public-name placeholder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureRenameTarget {
    /// Resolved fixture selection under the cursor.
    pub occurrence: FixtureOccurrence,

    /// Public fixture name shown by rename-capable clients.
    pub placeholder: String,
}

/// Enumerates statically resolved fixture occurrences in the current source.
pub(crate) fn fixture_occurrences(analysis: &SourceAnalysis) -> Vec<FixtureOccurrence> {
    let mut occurrences = Vec::new();

    occurrences.extend(
        analysis
            .fixture_model
            .local()
            .iter()
            .map(|definition| FixtureOccurrence {
                range: definition.public_name_range,
                edit_range: definition.public_name_edit_range,
                kind: FixtureOccurrenceKind::Definition,
                fixture: definition.id.clone(),
            }),
    );

    occurrences.extend(
        analysis
            .fixture_model
            .local()
            .iter()
            .flat_map(|definition| {
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
            }),
    );

    for function in &analysis.module.test_function_defs {
        occurrences.extend(function.parameters.iter_non_variadic_params().filter_map(
            |parameter| {
                let name = parameter.parameter.name.as_str();
                if !analysis.fixture_model.parameter_is_fixture(function, name) {
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
        occurrences.extend(body_fixture_occurrences(
            function,
            fixture_parameters(analysis, function),
        ));
    }

    for definition in analysis.fixture_model.local() {
        let Some(function) = analysis
            .module
            .fixture_function_defs
            .iter()
            .find(|function| function.name.range == definition.name_range)
        else {
            continue;
        };
        occurrences.extend(body_fixture_occurrences(
            function,
            definition.dependencies.iter().filter_map(|reference| {
                let FixtureResolution::Resolved(fixture) = &reference.resolution else {
                    return None;
                };
                Some((reference.name.clone(), fixture.clone()))
            }),
        ));
    }

    occurrences.sort_by_key(|occurrence| occurrence.range.start());
    occurrences
}

fn fixture_parameters(
    analysis: &SourceAnalysis,
    function: &StmtFunctionDef,
) -> impl Iterator<Item = (String, FixtureId)> {
    function
        .parameters
        .iter_non_variadic_params()
        .filter_map(|parameter| {
            let name = parameter.parameter.name.as_str();
            analysis
                .fixture_model
                .parameter_is_fixture(function, name)
                .then(|| {
                    resolve_source_fixture(analysis, name).map(|fixture| (name.to_owned(), fixture))
                })
                .flatten()
        })
}

fn body_fixture_occurrences(
    function: &StmtFunctionDef,
    parameters: impl IntoIterator<Item = (String, FixtureId)>,
) -> Vec<FixtureOccurrence> {
    let targets = parameters.into_iter().collect::<HashMap<_, _>>();
    if targets.is_empty() {
        return Vec::new();
    }

    let mut visitor = BodyReferenceVisitor {
        targets: &targets,
        shadowed: HashSet::new(),
        occurrences: Vec::new(),
        match_name: None,
        found: false,
        unsupported: false,
    };
    source_order::walk_body(&mut visitor, &function.body);
    visitor
        .occurrences
        .into_iter()
        .map(|(range, fixture)| FixtureOccurrence {
            range,
            edit_range: Some(range),
            kind: FixtureOccurrenceKind::BodyReference,
            fixture,
        })
        .collect()
}

struct BodyReferenceVisitor<'a> {
    targets: &'a HashMap<String, FixtureId>,
    shadowed: HashSet<String>,
    occurrences: Vec<(TextRange, FixtureId)>,
    match_name: Option<&'a str>,
    found: bool,
    unsupported: bool,
}

impl BodyReferenceVisitor<'_> {
    fn visit_nested_function(&mut self, function: &StmtFunctionDef) {
        for decorator in &function.decorator_list {
            self.visit_decorator(decorator);
        }
        self.visit_parameters(&function.parameters);
        if let Some(returns) = &function.returns {
            self.visit_annotation(returns);
        }

        let mut child = Self {
            targets: self.targets,
            shadowed: self.shadowed.clone(),
            occurrences: Vec::new(),
            match_name: self.match_name,
            found: false,
            unsupported: false,
        };
        let (global, nonlocal) = scope_directives(&function.body);
        if global
            .iter()
            .chain(&nonlocal)
            .any(|name| self.targets.contains_key(name))
        {
            child.unsupported = true;
        }
        child.shadowed.extend(local_bindings(function));
        child.shadowed.extend(global);
        source_order::walk_body(&mut child, &function.body);
        self.found |= child.found;
        self.unsupported |= child.unsupported;
        self.occurrences.extend(child.occurrences);
    }

    fn visit_lambda(&mut self, lambda: &ruff_python_ast::ExprLambda) {
        if let Some(parameters) = &lambda.parameters {
            self.visit_parameters(parameters);
        }
        let mut child = Self {
            targets: self.targets,
            shadowed: self.shadowed.clone(),
            occurrences: Vec::new(),
            match_name: self.match_name,
            found: false,
            unsupported: false,
        };
        if let Some(parameters) = &lambda.parameters {
            child.shadowed.extend(parameter_names(parameters));
        }
        child.shadowed.extend(expression_bindings(&lambda.body));
        child.visit_expr(&lambda.body);
        self.found |= child.found;
        self.unsupported |= child.unsupported;
        self.occurrences.extend(child.occurrences);
    }

    fn visit_comprehension_expression(&mut self, element: &Expr, generators: &[Comprehension]) {
        self.visit_comprehension_elements(&[element], generators);
    }

    fn visit_comprehension_elements(&mut self, elements: &[&Expr], generators: &[Comprehension]) {
        let Some(first) = generators.first() else {
            for element in elements {
                self.visit_expr(element);
            }
            return;
        };
        self.visit_expr(&first.iter);

        let mut child = Self {
            targets: self.targets,
            shadowed: self.shadowed.clone(),
            occurrences: Vec::new(),
            match_name: self.match_name,
            found: false,
            unsupported: false,
        };
        for (index, generator) in generators.iter().enumerate() {
            if index > 0 {
                child.visit_expr(&generator.iter);
            }
            child
                .shadowed
                .extend(comprehension_target_names(&generator.target));
            for condition in &generator.ifs {
                child.visit_expr(condition);
            }
        }
        for element in elements {
            child.visit_expr(element);
        }
        self.found |= child.found;
        self.unsupported |= child.unsupported;
        self.occurrences.extend(child.occurrences);
    }

    fn visit_nested_class(&mut self, class: &ruff_python_ast::StmtClassDef) {
        for decorator in &class.decorator_list {
            self.visit_decorator(decorator);
        }
        if let Some(type_params) = &class.type_params {
            self.visit_type_params(type_params);
        }
        if let Some(arguments) = &class.arguments {
            self.visit_arguments(arguments);
        }

        if class_bindings(class)
            .iter()
            .any(|name| self.targets.contains_key(name))
        {
            self.unsupported = true;
            return;
        }
        source_order::walk_body(self, &class.body);
    }
}

impl SourceOrderVisitor<'_> for BodyReferenceVisitor<'_> {
    fn visit_stmt(&mut self, statement: &'_ Stmt) {
        match statement {
            Stmt::FunctionDef(function) => self.visit_nested_function(function),
            Stmt::ClassDef(class) => self.visit_nested_class(class),
            Stmt::Global(global) => {
                if global
                    .names
                    .iter()
                    .any(|name| self.targets.contains_key(name.as_str()))
                {
                    self.unsupported = true;
                }
            }
            Stmt::Nonlocal(nonlocal) => {
                if nonlocal
                    .names
                    .iter()
                    .any(|name| self.targets.contains_key(name.as_str()))
                {
                    self.unsupported = true;
                }
                source_order::walk_stmt(self, statement);
            }
            _ => source_order::walk_stmt(self, statement),
        }
    }

    fn visit_expr(&mut self, expression: &'_ Expr) {
        if let Expr::Name(name) = expression {
            if !matches!(
                name.ctx,
                ExprContext::Load | ExprContext::Store | ExprContext::Del
            ) {
                return;
            }
            if !self.shadowed.contains(name.id.as_str())
                && let Some(fixture) = self.targets.get(name.id.as_str())
            {
                self.occurrences.push((name.range, fixture.clone()));
            }
            if !self.shadowed.contains(name.id.as_str())
                && self
                    .match_name
                    .is_some_and(|match_name| name.id == match_name)
            {
                self.found = true;
            }
        }

        match expression {
            Expr::Lambda(lambda) => self.visit_lambda(lambda),
            Expr::ListComp(comp) => {
                self.visit_comprehension_expression(&comp.elt, &comp.generators);
            }
            Expr::SetComp(comp) => self.visit_comprehension_expression(&comp.elt, &comp.generators),
            Expr::DictComp(comp) => {
                self.visit_comprehension_elements(&[&comp.key, &comp.value], &comp.generators);
            }
            Expr::Generator(comp) => {
                self.visit_comprehension_expression(&comp.elt, &comp.generators);
            }
            _ => source_order::walk_expr(self, expression),
        }
    }
}

pub(super) fn body_contains_name(function: &StmtFunctionDef, name: &str) -> bool {
    body_contains_name_in_scope(&function.body, name, HashSet::new())
}

fn body_contains_name_in_scope(body: &[Stmt], name: &str, shadowed: HashSet<String>) -> bool {
    let targets = HashMap::new();
    let mut visitor = BodyReferenceVisitor {
        targets: &targets,
        shadowed,
        occurrences: Vec::new(),
        match_name: Some(name),
        found: false,
        unsupported: false,
    };
    source_order::walk_body(&mut visitor, body);
    visitor.found
}

fn expression_contains_name_in_scope(
    expression: &Expr,
    name: &str,
    shadowed: HashSet<String>,
) -> bool {
    let targets = HashMap::new();
    let mut visitor = BodyReferenceVisitor {
        targets: &targets,
        shadowed,
        occurrences: Vec::new(),
        match_name: Some(name),
        found: false,
        unsupported: false,
    };
    visitor.visit_expr(expression);
    visitor.found
}

pub(super) fn body_has_nested_binding_conflict(
    function: &StmtFunctionDef,
    target_name: &str,
    new_name: &str,
) -> bool {
    let mut visitor = BindingConflictVisitor {
        target_name,
        new_name,
        shadowed: HashSet::new(),
        conflict: false,
    };
    source_order::walk_body(&mut visitor, &function.body);
    visitor.conflict
}

struct BindingConflictVisitor<'a> {
    target_name: &'a str,
    new_name: &'a str,
    shadowed: HashSet<String>,
    conflict: bool,
}

impl BindingConflictVisitor<'_> {
    fn visit_nested_scope(&mut self, body: &[Stmt], bindings: HashSet<String>) {
        if bindings.contains(self.new_name) {
            let mut shadowed = self.shadowed.clone();
            shadowed.extend(bindings.iter().cloned());
            if body_contains_name_in_scope(body, self.target_name, shadowed) {
                self.conflict = true;
            }
        }

        let mut child = Self {
            target_name: self.target_name,
            new_name: self.new_name,
            shadowed: self.shadowed.clone(),
            conflict: false,
        };
        child.shadowed.extend(bindings);
        source_order::walk_body(&mut child, body);
        self.conflict |= child.conflict;
    }
}

impl SourceOrderVisitor<'_> for BindingConflictVisitor<'_> {
    fn visit_stmt(&mut self, statement: &'_ Stmt) {
        match statement {
            Stmt::FunctionDef(function) => {
                self.visit_nested_scope(&function.body, local_bindings(function));
            }
            Stmt::ClassDef(class) => {
                self.visit_nested_scope(&class.body, class_bindings(class));
            }
            _ => source_order::walk_stmt(self, statement),
        }
    }

    fn visit_expr(&mut self, expression: &'_ Expr) {
        if let Expr::Lambda(lambda) = expression {
            let mut bindings = HashSet::new();
            if let Some(parameters) = &lambda.parameters {
                bindings.extend(parameter_names(parameters));
            }
            bindings.extend(expression_bindings(&lambda.body));
            if bindings.contains(self.new_name) {
                let mut shadowed = self.shadowed.clone();
                shadowed.extend(bindings.iter().cloned());
                if expression_contains_name_in_scope(&lambda.body, self.target_name, shadowed) {
                    self.conflict = true;
                }
            }
            let mut child = Self {
                target_name: self.target_name,
                new_name: self.new_name,
                shadowed: self.shadowed.clone(),
                conflict: false,
            };
            child.shadowed.extend(bindings);
            child.visit_expr(&lambda.body);
            self.conflict |= child.conflict;
        } else {
            source_order::walk_expr(self, expression);
        }
    }
}

pub(super) fn body_has_unsupported_bindings(
    function: &StmtFunctionDef,
    target_names: &HashSet<String>,
) -> bool {
    let targets = target_names
        .iter()
        .map(|name| {
            (
                name.clone(),
                FixtureId {
                    path: "".into(),
                    range: TextRange::default(),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let mut visitor = BodyReferenceVisitor {
        targets: &targets,
        shadowed: HashSet::new(),
        occurrences: Vec::new(),
        match_name: None,
        found: false,
        unsupported: false,
    };
    source_order::walk_body(&mut visitor, &function.body);
    visitor.unsupported
}

fn parameter_names(parameters: &ruff_python_ast::Parameters) -> impl Iterator<Item = String> + '_ {
    parameters
        .iter()
        .map(|parameter| parameter.name().as_str().to_owned())
}

fn expression_bindings(expression: &Expr) -> HashSet<String> {
    let mut visitor = BindingVisitor {
        bindings: HashSet::new(),
    };
    source_order::walk_expr(&mut visitor, expression);
    visitor.bindings
}

fn comprehension_target_names(expression: &Expr) -> HashSet<String> {
    let mut visitor = NameCollector {
        names: HashSet::new(),
    };
    if let Expr::Name(name) = expression {
        if matches!(name.ctx, ExprContext::Store | ExprContext::Del) {
            visitor.names.insert(name.id.to_string());
        }
    } else {
        source_order::walk_expr(&mut visitor, expression);
    }
    visitor.names
}

struct NameCollector {
    names: HashSet<String>,
}

impl SourceOrderVisitor<'_> for NameCollector {
    fn visit_expr(&mut self, expression: &'_ Expr) {
        if let Expr::Name(name) = expression
            && matches!(name.ctx, ExprContext::Store | ExprContext::Del)
        {
            self.names.insert(name.id.to_string());
        } else {
            source_order::walk_expr(self, expression);
        }
    }
}

fn local_bindings(function: &StmtFunctionDef) -> HashSet<String> {
    let mut visitor = BindingVisitor {
        bindings: parameter_names(&function.parameters).collect(),
    };
    source_order::walk_body(&mut visitor, &function.body);
    visitor.bindings
}

fn class_bindings(class: &ruff_python_ast::StmtClassDef) -> HashSet<String> {
    let mut visitor = BindingVisitor {
        bindings: HashSet::new(),
    };
    source_order::walk_body(&mut visitor, &class.body);
    visitor.bindings
}

fn scope_directives(body: &[Stmt]) -> (HashSet<String>, HashSet<String>) {
    let mut visitor = ScopeDirectiveVisitor {
        global: HashSet::new(),
        nonlocal: HashSet::new(),
    };
    source_order::walk_body(&mut visitor, body);
    (visitor.global, visitor.nonlocal)
}

struct ScopeDirectiveVisitor {
    global: HashSet<String>,
    nonlocal: HashSet<String>,
}

impl SourceOrderVisitor<'_> for ScopeDirectiveVisitor {
    fn visit_stmt(&mut self, statement: &'_ Stmt) {
        match statement {
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            Stmt::Global(global) => self
                .global
                .extend(global.names.iter().map(ToString::to_string)),
            Stmt::Nonlocal(nonlocal) => self
                .nonlocal
                .extend(nonlocal.names.iter().map(ToString::to_string)),
            _ => source_order::walk_stmt(self, statement),
        }
    }

    fn visit_expr(&mut self, expression: &'_ Expr) {
        if !matches!(expression, Expr::Lambda(_)) {
            source_order::walk_expr(self, expression);
        }
    }
}

struct BindingVisitor {
    bindings: HashSet<String>,
}

impl SourceOrderVisitor<'_> for BindingVisitor {
    fn visit_stmt(&mut self, statement: &'_ Stmt) {
        match statement {
            Stmt::FunctionDef(function) => {
                self.bindings.insert(function.name.to_string());
            }
            Stmt::ClassDef(class) => {
                self.bindings.insert(class.name.to_string());
            }
            Stmt::Global(_) | Stmt::Nonlocal(_) => {}
            _ => source_order::walk_stmt(self, statement),
        }
    }

    fn visit_expr(&mut self, expression: &'_ Expr) {
        if let Expr::Name(name) = expression
            && matches!(name.ctx, ExprContext::Store | ExprContext::Del)
        {
            self.bindings.insert(name.id.to_string());
        }
        if !matches!(
            expression,
            Expr::Lambda(_)
                | Expr::ListComp(_)
                | Expr::SetComp(_)
                | Expr::DictComp(_)
                | Expr::Generator(_)
        ) {
            source_order::walk_expr(self, expression);
        }
    }

    fn visit_alias(&mut self, alias: &'_ ruff_python_ast::Alias) {
        self.bindings.insert(
            alias
                .asname
                .as_ref()
                .map_or_else(
                    || alias.name.as_str().split('.').next().unwrap_or_default(),
                    |name| name.as_str(),
                )
                .to_owned(),
        );
    }

    fn visit_except_handler(&mut self, handler: &'_ ruff_python_ast::ExceptHandler) {
        if let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler
            && let Some(name) = &handler.name
        {
            self.bindings.insert(name.to_string());
        }
        source_order::walk_except_handler(self, handler);
    }
}

/// Returns the resolved fixture occurrence containing `offset`, if any.
pub(super) fn fixture_occurrence(
    analysis: &SourceAnalysis,
    offset: TextSize,
) -> Option<FixtureOccurrence> {
    fixture_occurrences(analysis)
        .into_iter()
        .find(|occurrence| occurrence.range.contains_inclusive(offset))
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
                .fixture_model
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
        .fixture_model
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
        .fixture_model
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
        .fixture_model
        .definition(fixture)
        .map(|definition| definition.name.clone())
}

fn resolve_source_fixture(analysis: &SourceAnalysis, name: &str) -> Option<FixtureId> {
    analysis
        .fixture_model
        .resolve(name)
        .map(|fixture| fixture.id.clone())
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
            analysis
                .fixture_model
                .is_use_fixtures_reference(&call.func)
                .then_some(call)
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
    fn enumerates_body_references_and_respects_nested_bindings() {
        let source = "from karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\ndef test_example(database):\n    direct = database\n    def closure():\n        return database\n    def shadowed(database):\n        return database\n    return direct, closure\n";
        let occurrences = fixture_occurrences(&analysis(source));
        let body_references = occurrences
            .iter()
            .filter(|occurrence| occurrence.kind == FixtureOccurrenceKind::BodyReference)
            .collect::<Vec<_>>();

        assert_eq!(body_references.len(), 2);
        assert_eq!(
            body_references
                .iter()
                .map(|occurrence| &source[occurrence.range.to_std_range()])
                .collect::<Vec<_>>(),
            ["database", "database"]
        );
        assert!(
            body_references
                .iter()
                .all(|occurrence| occurrence.edit_range.is_some())
        );
    }

    #[test]
    fn enumerates_fixture_provider_body_references() {
        let source = "from karva import fixture\n@fixture\ndef database(): pass\n@fixture\ndef wrapper(database):\n    return database\n";
        let occurrences = fixture_occurrences(&analysis(source));
        assert_eq!(
            occurrences
                .iter()
                .filter(|occurrence| occurrence.kind == FixtureOccurrenceKind::BodyReference)
                .count(),
            1
        );
    }

    #[test]
    fn respects_comprehension_bindings() {
        let source = "from karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\ndef test_example(database):\n    values = [database for database in [database]]\n    return database, values\n";
        let occurrences = fixture_occurrences(&analysis(source));
        let body_references = occurrences
            .iter()
            .filter(|occurrence| occurrence.kind == FixtureOccurrenceKind::BodyReference)
            .collect::<Vec<_>>();

        assert_eq!(body_references.len(), 2);
        assert!(
            body_references
                .iter()
                .all(|occurrence| { &source[occurrence.range.to_std_range()] == "database" })
        );
    }

    #[test]
    fn respects_lambda_bindings() {
        let source = "from karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\ndef test_example(database):\n    local = lambda: (database := object())\n    return database, local\n";
        let occurrences = fixture_occurrences(&analysis(source));
        assert_eq!(
            occurrences
                .iter()
                .filter(|occurrence| occurrence.kind == FixtureOccurrenceKind::BodyReference)
                .count(),
            1
        );
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
        let source = "from karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\ndef test_example(database): pass\n";
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
        let root_source = "from karva import fixture\n@fixture\ndef database(): pass\n";
        let nested_source = "from karva import fixture\n@fixture\ndef database(): pass\n";
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
            "from karva import fixture\n@fixture\ndef database(): pass\n".to_owned(),
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
