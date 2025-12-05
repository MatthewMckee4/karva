use std::collections::HashMap;

use camino::Utf8PathBuf;
use pyo3::{
    prelude::*,
    types::{PyDict, PyIterator},
};

use crate::{
    Context, FunctionKind, IndividualTestResultKind,
    diagnostic::{
        report_fixture_failure, report_missing_fixtures, report_test_failure,
        report_test_pass_on_expect_failure,
    },
    extensions::{
        fixtures::{
            Finalizer, FixtureRequest, FixtureScope, FunctionNode, ModuleNode,
            NormalizedFixture, RequiresFixtures, create_fixture_with_finalizer,
            missing_arguments_from_error,
        },
        tags::{
            expect_fail::ExpectFailTag,
            skip::{extract_skip_reason, is_skip_exception},
        },
    },
    normalize::{NormalizedModule, NormalizedPackage, NormalizedTest},
    runner::{FinalizerCache, FixtureCache},
    utils::{full_test_name, source_file},
};

/// A struct that is used to execute tests within a package.
///
/// We assume a normalized state of the package.
pub struct NormalizedPackageRunner<'ctx, 'proj, 'rep> {
    context: &'ctx Context<'proj, 'rep>,
    fixture_cache: FixtureCache,
    finalizer_cache: FinalizerCache,
}

impl<'ctx, 'proj, 'rep> NormalizedPackageRunner<'ctx, 'proj, 'rep> {
    pub(crate) fn new(context: &'ctx Context<'proj, 'rep>) -> Self {
        Self {
            context,
            fixture_cache: FixtureCache::default(),
            finalizer_cache: FinalizerCache::default(),
        }
    }

    /// Executes all tests in a package.
    ///
    /// The main entrypoint for actual test execution.
    pub(crate) fn execute(&self, py: Python<'_>, session: NormalizedPackage) {
        self.execute_fixtures(py, &session.auto_use_fixtures);

        self.execute_package(py, session);

        self.clean_up_scope(py, FixtureScope::Session);
    }

    /// Execute a module.
    ///
    /// Executes all tests in a module.
    ///
    /// Failing fast if the user has specified that we should.
    fn execute_module(&self, py: Python<'_>, module: NormalizedModule) -> bool {
        self.execute_fixtures(py, &module.auto_use_fixtures);

        let mut passed = true;

        for test_function in module.test_functions {
            passed &= self.execute_test(py, test_function);

            if self.context.project().settings().test().fail_fast && !passed {
                break;
            }
        }

        self.clean_up_scope(py, FixtureScope::Module);

        passed
    }

    /// Execute a package.
    ///
    /// Executes all tests in each module and sub-package.
    ///
    /// Failing fast if the user has specified that we should.
    fn execute_package(&self, py: Python<'_>, package: NormalizedPackage) -> bool {
        let NormalizedPackage {
            modules,
            packages,
            auto_use_fixtures,
        } = package;

        self.execute_fixtures(py, &auto_use_fixtures);

        let mut passed = true;

        for module in modules.into_values() {
            passed &= self.execute_module(py, module);

            if self.context.project().settings().test().fail_fast && !passed {
                break;
            }
        }

        if !self.context.project().settings().test().fail_fast || passed {
            for sub_package in packages.into_values() {
                passed &= self.execute_package(py, sub_package);

                if self.context.project().settings().test().fail_fast && !passed {
                    break;
                }
            }
        }

        self.clean_up_scope(py, FixtureScope::Package);

        passed
    }

    /// Run a normalized test function.
    fn execute_test(&self, py: Python<'_>, test: NormalizedTest) -> bool {
        let tags = test.resolved_tags();
        let test_module_path = test.module_path().clone();

        let NormalizedTest {
            name,
            params,
            fixture_dependencies,
            use_fixture_dependencies,
            auto_use_fixtures,
            function,
            tags: _,
            stmt_function_def,
        } = test;

        if let (true, reason) = tags.should_skip() {
            return self.context.register_test_case_result(
                &name.to_string(),
                IndividualTestResultKind::Skipped { reason },
            );
        }

        let expect_fail_tag = tags.expect_fail_tag();
        let expect_fail = expect_fail_tag
            .as_ref()
            .is_some_and(ExpectFailTag::should_expect_fail);

        let mut test_finalizers = Vec::new();

        self.execute_fixtures(py, &use_fixture_dependencies);

        let mut test_arguments = HashMap::new();

        for fixture in fixture_dependencies.iter() {
            if let Some((value, finalizer)) = self.execute_fixture_with_context(
                py,
                fixture,
                Some(&function),
                Some(&test_module_path),
            ) {
                test_arguments.insert(fixture.function_name().to_string(), value);
                if let Some(finalizer) = finalizer {
                    test_finalizers.push(finalizer);
                }
            }
        }

        self.execute_fixtures(py, &auto_use_fixtures);

        for (key, value) in params {
            test_arguments.insert(key, value);
        }

        // Note: Test functions can't directly request the 'request' fixture
        // Only fixtures can request 'request'

        let full_test_name = full_test_name(py, name.to_string(), &test_arguments);

        tracing::info!("Running test `{}`", full_test_name);

        let test_result = if test_arguments.is_empty() {
            function.call0(py)
        } else {
            let py_dict = PyDict::new(py);
            for (key, value) in &test_arguments {
                let _ = py_dict.set_item(key, value);
            }
            function.call(py, (), Some(&py_dict))
        };

        let passed = match test_result {
            Ok(_) => {
                if expect_fail {
                    let reason = expect_fail_tag.and_then(|tag| tag.reason());

                    report_test_pass_on_expect_failure(
                        self.context,
                        source_file(self.context.db().system(), &test_module_path),
                        &stmt_function_def,
                        reason,
                    );

                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Failed,
                    )
                } else {
                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Passed,
                    )
                }
            }
            Err(err) => {
                if is_skip_exception(py, &err) {
                    let reason = extract_skip_reason(py, &err);
                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Skipped { reason },
                    )
                } else if expect_fail {
                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Passed,
                    )
                } else {
                    let missing_args =
                        missing_arguments_from_error(name.function_name(), &err.to_string());

                    if missing_args.is_empty() {
                        report_test_failure(
                            self.context,
                            py,
                            source_file(self.context.db().system(), &test_module_path),
                            &stmt_function_def,
                            &test_arguments,
                            &err,
                        );
                    } else {
                        report_missing_fixtures(
                            self.context,
                            source_file(self.context.db().system(), &test_module_path),
                            &stmt_function_def,
                            &missing_args,
                            FunctionKind::Test,
                        );
                    }

                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Failed,
                    )
                }
            }
        };

        for finalizer in test_finalizers.into_iter().rev() {
            finalizer.run(self.context, py);
        }

        self.clean_up_scope(py, FixtureScope::Function);

        passed
    }

    /// Execute a fixture
    fn execute_fixture(
        &self,
        py: Python<'_>,
        fixture: &NormalizedFixture,
    ) -> Option<(Py<PyAny>, Option<Finalizer>)> {
        self.execute_fixture_with_context(py, fixture, None, None)
    }

    /// Execute a fixture with optional test context
    fn execute_fixture_with_context(
        &self,
        py: Python<'_>,
        fixture: &NormalizedFixture,
        test_function: Option<&Py<PyAny>>,
        module_path: Option<&Utf8PathBuf>,
    ) -> Option<(Py<PyAny>, Option<Finalizer>)> {
        if let Some(cached) = self
            .fixture_cache
            .get(fixture.function_name(), fixture.scope())
        {
            return Some((cached, None));
        }

        let mut fixture_arguments = HashMap::new();

        for dep_fixture in fixture.dependencies() {
            let (value, finalizer) =
                self.execute_fixture_with_context(py, dep_fixture, test_function, module_path)?;

            // Dependency finalizers are always added to the cache
            // They need to outlive the current fixture execution
            if let Some(finalizer) = finalizer {
                self.finalizer_cache.add_finalizer(finalizer);
            }

            fixture_arguments.insert(dep_fixture.function_name().to_string(), value);
        }

        // Check if this fixture requires the 'request' fixture
        if let Some(user_defined) = fixture.as_user_defined() {
            let required = user_defined.stmt_function_def.required_fixtures(py);
            if required.contains(&"request".to_string()) {
                if let Ok(request_obj) =
                    create_fixture_request(py, fixture, test_function, module_path)
                {
                    fixture_arguments.insert("request".to_string(), request_obj.into_any());
                }
            }
        }

        let fixture_call_result = match fixture.call(py, &fixture_arguments) {
            Ok(fixture_call_result) => fixture_call_result,
            Err(err) => {
                handle_fixture_error(py, self.context, fixture, &fixture_arguments, &err);
                return None;
            }
        };

        let (final_result, finalizer) =
            match get_value_and_finalizer(py, fixture, fixture_call_result) {
                Ok((final_result, finalizer)) => (final_result, finalizer),
                Err(Some(err)) => {
                    handle_fixture_error(py, self.context, fixture, &fixture_arguments, &err);
                    return None;
                }
                Err(None) => {
                    return None;
                }
            };

        // Cache the result
        self.fixture_cache.insert(
            fixture.function_name().to_string(),
            final_result.clone(),
            fixture.scope(),
        );

        // Handle finalizer based on scope
        // Function-scoped finalizers are returned to be run immediately after the test
        // Higher-scoped finalizers are added to the cache
        let return_finalizer = finalizer.map_or_else(
            || None,
            |f| {
                if f.scope == FixtureScope::Function {
                    Some(f)
                } else {
                    self.finalizer_cache.add_finalizer(f);
                    None
                }
            },
        );

        Some((final_result, return_finalizer))
    }

    /// Cleans up the fixtures and finalizers for a given scope.
    ///
    /// This should be run after the given scope has finished execution.
    fn clean_up_scope(&self, py: Python, scope: FixtureScope) {
        self.finalizer_cache
            .run_and_clear_scope(self.context, py, scope);

        self.fixture_cache.clear_fixtures(scope);
    }

    /// Executes the fixtures for a given scope.
    ///
    /// Helper function used at the beginning of a scope to execute auto use fixture.
    fn execute_fixtures(&self, py: Python, fixture: &[NormalizedFixture]) {
        for fixture in fixture {
            if let Some((_, Some(finalizer))) = self.execute_fixture(py, fixture) {
                self.finalizer_cache.add_finalizer(finalizer);
            }
        }
    }
}

/// Creates a `FixtureRequest` object with the appropriate node based on scope
fn create_fixture_request(
    py: Python<'_>,
    fixture: &NormalizedFixture,
    test_function: Option<&Py<PyAny>>,
    module_path: Option<&Utf8PathBuf>,
) -> PyResult<Py<FixtureRequest>> {
    let param_value = fixture
        .param()
        .and_then(|param| param.values.first())
        .cloned();

    // Determine the node based on the fixture scope
    let node = match fixture.scope() {
        FixtureScope::Function => {
            // For function scope, node is the test function
            if let Some(test_fn) = test_function {
                // Wrap the test function in FunctionNode to provide .name attribute
                if let Ok(fn_node) = Py::new(py, FunctionNode::new(test_fn.clone())) {
                    fn_node.into_any()
                } else {
                    test_fn.clone()
                }
            } else {
                get_fixture_function_node(py, fixture)
            }
        }
        FixtureScope::Module => {
            // For module scope, node should be the module
            // For now, use the fixture function as a placeholder
            get_module_node(py, module_path, fixture)
        }
        FixtureScope::Package => {
            // For package scope, node should be the package
            // For now, use the fixture function as a placeholder
            get_package_node(py, module_path, fixture)
        }
        FixtureScope::Session => {
            // For session scope, node should be the session
            // For now, use the fixture function as a placeholder
            get_fixture_function_node(py, fixture)
        }
    };

    Py::new(py, FixtureRequest::new(param_value, node))
}

/// Gets the fixture function as the node (fallback behavior)
fn get_fixture_function_node(py: Python<'_>, fixture: &NormalizedFixture) -> Py<PyAny> {
    let function = match fixture {
        NormalizedFixture::UserDefined(user_defined) => user_defined.py_function.clone(),
        NormalizedFixture::BuiltIn(builtin) => builtin.py_value.clone(),
    };

    // Wrap the function in a FunctionNode to provide .name attribute
    if let Ok(node) = Py::new(py, FunctionNode::new(function.clone())) {
        return node.into_any();
    }

    // Fallback to raw function if wrapper creation fails
    function
}

/// Gets the module node for module-scoped fixtures
fn get_module_node(
    py: Python<'_>,
    module_path: Option<&Utf8PathBuf>,
    fixture: &NormalizedFixture,
) -> Py<PyAny> {
    // For module-scoped fixtures, return the module object where the test is running
    // First try to use the test's module path (if provided)
    let path_to_use = module_path.or_else(|| {
        // Fall back to the fixture's module path if no test context
        fixture.as_user_defined().map(|f| f.module_path())
    });

    if let Some(path) = path_to_use {
        // Convert file path to module name by removing .py extension and converting slashes
        if let Some(file_stem) = path.file_stem() {
            // Try to find the module in sys.modules
            if let Ok(sys) = py.import("sys") {
                if let Ok(modules) = sys.getattr("modules") {
                    // First try just the file stem (common case for test files in the same directory)
                    if let Ok(module) = modules.get_item(file_stem) {
                        let module_py: Py<PyAny> = module.into();
                        // Wrap the module in a ModuleNode to provide .name attribute
                        if let Ok(node) = Py::new(py, ModuleNode::new(module_py.clone())) {
                            return node.into_any();
                        }
                        return module_py;
                    }

                    // If that didn't work, try to find a module that ends with this file stem
                    // This handles cases where the module has a package prefix
                    if let Ok(dict) = modules.cast_into::<pyo3::types::PyDict>() {
                        for (key, value) in dict.iter() {
                            if let Ok(key_str) = key.extract::<String>() {
                                // Check if the key ends with the file stem
                                // e.g., "tests.test_foo" ends with "test_foo"
                                if key_str.ends_with(file_stem)
                                    && (key_str == file_stem
                                        || key_str.ends_with(&format!(".{}", file_stem)))
                                {
                                    let value_py: Py<PyAny> = value.into();
                                    // Wrap the module in a ModuleNode to provide .name attribute
                                    if let Ok(node) = Py::new(py, ModuleNode::new(value_py.clone()))
                                    {
                                        return node.into_any();
                                    }
                                    return value_py;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback to fixture function if we couldn't find the module
    get_fixture_function_node(py, fixture)
}

/// Gets the package node for package-scoped fixtures
fn get_package_node(
    py: Python<'_>,
    _module_path: Option<&Utf8PathBuf>,
    fixture: &NormalizedFixture,
) -> Py<PyAny> {
    // TODO: Return the actual package object
    // For now, return the fixture function
    get_fixture_function_node(py, fixture)
}

fn handle_fixture_error(
    py: Python,
    context: &Context,
    fixture: &NormalizedFixture,
    fixture_arguments: &HashMap<String, Py<PyAny>>,
    err: &PyErr,
) {
    let Some(fixture) = fixture.as_user_defined() else {
        // Assume that builtin fixtures don't fail
        return;
    };

    let missing_args = missing_arguments_from_error(fixture.name.function_name(), &err.to_string());

    if missing_args.is_empty() {
        report_fixture_failure(
            context,
            py,
            source_file(context.db().system(), fixture.module_path()),
            &fixture.stmt_function_def,
            fixture_arguments,
            err,
        );
    } else {
        report_missing_fixtures(
            context,
            source_file(context.db().system(), fixture.module_path()),
            &fixture.stmt_function_def,
            &missing_args,
            FunctionKind::Fixture,
        );
    }
}

fn get_value_and_finalizer(
    py: Python<'_>,
    fixture: &NormalizedFixture,
    fixture_call_result: Py<PyAny>,
) -> Result<(Py<PyAny>, Option<Finalizer>), Option<PyErr>> {
    // If this is a generator fixture, we need to call next() to get the actual value
    // and create a finalizer for cleanup
    if let Some(user_defined_fixture) = fixture.as_user_defined()
        && user_defined_fixture.is_generator
        && let Ok(mut bound_iterator) = fixture_call_result
            .clone()
            .into_bound(py)
            .cast_into::<PyIterator>()
    {
        match bound_iterator.next() {
            Some(Ok(value)) => {
                let py_iter = bound_iterator.clone().unbind();
                let finalizer = {
                    Finalizer {
                        fixture_return: py_iter,
                        scope: fixture.scope(),
                        fixture_name: Some(user_defined_fixture.name.clone()),
                        stmt_function_def: Some(user_defined_fixture.stmt_function_def.clone()),
                    }
                };

                Ok((value.unbind(), Some(finalizer)))
            }
            Some(Err(err)) => Err(Some(err)),
            None => Err(None),
        }
    } else if let Some(builtin_fixture) = fixture.as_builtin()
        && let Some(finalizer_fn) = &builtin_fixture.finalizer
        && let Ok(mut bound_iterator) =
            create_fixture_with_finalizer(py, &fixture_call_result, finalizer_fn)
        && let Some(Ok(value)) = bound_iterator.next()
    {
        let py_iter_unbound = bound_iterator.clone().unbind();
        let finalizer = Finalizer {
            fixture_return: py_iter_unbound,
            scope: builtin_fixture.scope,
            fixture_name: None,
            stmt_function_def: None,
        };

        Ok((value.unbind(), Some(finalizer)))
    } else {
        Ok((fixture_call_result, None))
    }
}
