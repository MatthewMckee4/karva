use std::collections::HashMap;

use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::extensions::tags::python::{PyTag, PyTestFunction};

pub mod python;

/// Represents a decorator function in Python that can be used to extend the functionality of a test.
#[derive(Debug, Clone)]
pub(crate) enum Tag {
    Parametrize(ParametrizeTag),
    UseFixtures(UseFixturesTag),
    Skip(SkipTag),
}

impl Tag {
    /// Converts a Python Tag into an internal Tag.
    #[must_use]
    pub(crate) fn from_py_tag(py_tag: &PyTag) -> Self {
        match py_tag {
            PyTag::Parametrize {
                arg_names,
                arg_values,
            } => Self::Parametrize(ParametrizeTag {
                arg_names: arg_names.clone(),
                arg_values: arg_values.clone(),
            }),
            PyTag::UseFixtures { fixture_names } => Self::UseFixtures(UseFixturesTag {
                fixture_names: fixture_names.clone(),
            }),
            PyTag::Skip { reason } => Self::Skip(SkipTag {
                reason: reason.clone(),
            }),
        }
    }

    /// Converts a Pytest mark into an internal Tag.
    ///
    /// This is used to allow Pytest marks to be used as Karva tags.
    #[must_use]
    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let name = py_mark.getattr("name").ok()?.extract::<String>().ok()?;
        match name.as_str() {
            "parametrize" => ParametrizeTag::try_from_pytest_mark(py_mark).map(Self::Parametrize),
            "usefixtures" => UseFixturesTag::try_from_pytest_mark(py_mark).map(Self::UseFixtures),
            "skip" => SkipTag::try_from_pytest_mark(py_mark).map(Self::Skip),
            _ => None,
        }
    }
}

/// Represents different argument names and values that can be given to a test.
///
/// This is most useful to repeat a test multiple times with different arguments instead of duplicating the test.
#[derive(Debug, Clone)]
pub(crate) struct ParametrizeTag {
    /// The names of the arguments
    ///
    /// These are used as keyword argument names for the test function.
    arg_names: Vec<String>,

    /// The values associated with each argument name.
    arg_values: Vec<Vec<Py<PyAny>>>,
}

impl ParametrizeTag {
    #[must_use]
    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let args = py_mark.getattr("args").ok()?;
        if let Ok((arg_name, arg_values)) = args.extract::<(String, Vec<Py<PyAny>>)>() {
            Some(Self {
                arg_names: vec![arg_name],
                arg_values: arg_values.into_iter().map(|v| vec![v]).collect(),
            })
        } else if let Ok((arg_names, arg_values)) =
            args.extract::<(Vec<String>, Vec<Vec<Py<PyAny>>>)>()
        {
            Some(Self {
                arg_names,
                arg_values,
            })
        } else {
            None
        }
    }

    /// Returns each parameterize case.
    ///
    /// Each [`HashMap`] is used as keyword arguments for the test function.
    #[must_use]
    pub(crate) fn each_arg_value(&self) -> Vec<HashMap<String, Py<PyAny>>> {
        let total_combinations = self.arg_values.len();
        let mut param_args = Vec::with_capacity(total_combinations);

        for values in &self.arg_values {
            let mut current_parameratisation = HashMap::with_capacity(self.arg_names.len());
            for (arg_name, arg_value) in self.arg_names.iter().zip(values.iter()) {
                current_parameratisation.insert(arg_name.clone(), arg_value.clone());
            }
            param_args.push(current_parameratisation);
        }
        param_args
    }
}

/// Represents required fixtures that should be called before a test function is run.
///
/// These fixtures are not specified as arguments as the function does not directly need them.
/// But they are still called.
#[derive(Debug, Clone)]
pub(crate) struct UseFixturesTag {
    /// The names of the fixtures to be called.
    fixture_names: Vec<String>,
}

impl UseFixturesTag {
    #[must_use]
    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let args = py_mark.getattr("args").ok()?;
        args.extract::<Vec<String>>()
            .map_or(None, |fixture_names| Some(Self { fixture_names }))
    }

    #[must_use]
    pub(crate) fn fixture_names(&self) -> &[String] {
        &self.fixture_names
    }
}

/// Represents a test that should be skipped.
///
/// A given reason will be logged if given.
#[derive(Debug, Clone)]
pub(crate) struct SkipTag {
    reason: Option<String>,
}

impl SkipTag {
    #[must_use]
    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let kwargs = py_mark.getattr("kwargs").ok()?;

        if let Ok(reason) = kwargs.get_item("reason") {
            if let Ok(reason_str) = reason.extract::<String>() {
                return Some(Self {
                    reason: Some(reason_str),
                });
            }
        }

        let args = py_mark.getattr("args").ok()?;

        if let Ok(args_tuple) = args.extract::<(String,)>() {
            return Some(Self {
                reason: Some(args_tuple.0),
            });
        }

        Some(Self { reason: None })
    }

    #[must_use]
    pub(crate) fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

/// Represents a collection of tags associated with a test function.
///
/// This means we can collect tags and use them all for the same function.
#[derive(Debug, Clone, Default)]
pub(crate) struct Tags {
    inner: Vec<Tag>,
}

impl Tags {
    #[must_use]
    pub(crate) fn from_py_any(
        py: Python<'_>,
        py_function: &Py<PyAny>,
        function_definition: Option<&StmtFunctionDef>,
    ) -> Self {
        if function_definition.is_some_and(|def| def.decorator_list.is_empty()) {
            return Self::default();
        }

        if let Ok(py_test_function) = py_function.extract::<Py<PyTestFunction>>(py) {
            let mut tags = Vec::new();
            for tag in &py_test_function.borrow(py).tags.inner {
                tags.push(Tag::from_py_tag(tag));
            }
            return Self { inner: tags };
        } else if let Ok(wrapped) = py_function.getattr(py, "__wrapped__") {
            if let Ok(py_wrapped_function) = wrapped.extract::<Py<PyTestFunction>>(py) {
                let mut tags = Vec::new();
                for tag in &py_wrapped_function.borrow(py).tags.inner {
                    tags.push(Tag::from_py_tag(tag));
                }
                return Self { inner: tags };
            }
        }

        if let Some(tags) = Self::from_pytest_function(py, py_function) {
            return tags;
        }

        Self::default()
    }

    #[must_use]
    pub(crate) fn from_pytest_function(
        py: Python<'_>,
        py_test_function: &Py<PyAny>,
    ) -> Option<Self> {
        let mut tags = Vec::new();
        if let Ok(marks) = py_test_function.getattr(py, "pytestmark") {
            if let Ok(marks_list) = marks.extract::<Vec<Bound<'_, PyAny>>>(py) {
                for mark in marks_list {
                    if let Some(tag) = Tag::try_from_pytest_mark(&mark) {
                        tags.push(tag);
                    }
                }
            }
        } else {
            return None;
        }
        Some(Self { inner: tags })
    }

    /// Return all parametrizations
    ///
    /// This function ensures that if we have multiple parametrize tags, we combine them together.
    #[must_use]
    pub(crate) fn parametrize_args(&self) -> Vec<HashMap<String, Py<PyAny>>> {
        let mut param_args: Vec<HashMap<String, Py<PyAny>>> = vec![HashMap::new()];

        for tag in &self.inner {
            if let Tag::Parametrize(parametrize_tag) = tag {
                let current_values = parametrize_tag.each_arg_value();
                let mut new_param_args =
                    Vec::with_capacity(param_args.len() * current_values.len());
                for existing_params in &param_args {
                    for new_params in &current_values {
                        let mut combined_params = existing_params.clone();
                        combined_params.extend(new_params.clone());
                        new_param_args.push(combined_params);
                    }
                }
                param_args = new_param_args;
            }
        }
        param_args
    }

    /// Get all required fixture names for the given test.
    #[must_use]
    pub(crate) fn required_fixtures_names(&self) -> Vec<String> {
        let mut fixture_names = Vec::new();
        for tag in &self.inner {
            if let Tag::UseFixtures(use_fixtures_tag) = tag {
                fixture_names.extend_from_slice(use_fixtures_tag.fixture_names());
            }
        }
        fixture_names
    }

    /// Return the skip tag if it exists.
    #[must_use]
    pub(crate) fn skip_tag(&self) -> Option<SkipTag> {
        for tag in &self.inner {
            if let Tag::Skip(skip_tag) = tag {
                return Some(skip_tag.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, ffi::CString};

    use pyo3::{prelude::*, types::PyDict};
    use rstest::rstest;

    use super::*;

    fn get_parametrize_decorator(framework: &str) -> &'static str {
        match framework {
            "karva" => "@karva.tags.parametrize",
            "pytest" => "@pytest.mark.parametrize",
            _ => panic!("Unsupported framework: {framework}"),
        }
    }

    fn get_usefixtures_decorator(framework: &str) -> &'static str {
        match framework {
            "karva" => "@karva.tags.use_fixtures",
            "pytest" => "@pytest.mark.usefixtures",
            _ => panic!("Unsupported framework: {framework}"),
        }
    }

    fn get_skip_decorator(framework: &str) -> &'static str {
        match framework {
            "karva" => "@karva.tags.skip",
            "pytest" => "@pytest.mark.skip",
            _ => panic!("Unsupported framework: {framework}"),
        }
    }

    #[rstest]
    fn test_parametrize_args_single_arg(#[values("karva", "pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r#"
import {}

{}("arg1", [1, 2, 3])
def test_parametrize(arg1):
    pass
                "#,
                framework,
                get_parametrize_decorator(framework)
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_parametrize").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            let expected_parametrize_args = [
                HashMap::from([(String::from("arg1"), 1)]),
                HashMap::from([(String::from("arg1"), 2)]),
                HashMap::from([(String::from("arg1"), 3)]),
            ];

            for (i, parametrize_arg) in tags.parametrize_args().iter().enumerate() {
                for (key, value) in parametrize_arg {
                    assert_eq!(
                        value.extract::<i32>(py).unwrap(),
                        expected_parametrize_args[i][&key.to_string()]
                    );
                }
            }
        });
    }

    #[rstest]
    fn test_parametrize_args_two_args(#[values("karva", "pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r#"
import {}

{}(("arg1", "arg2"), [(1, 4), (2, 5), (3, 6)])
def test_parametrize(arg1, arg2):
    pass
                "#,
                framework,
                get_parametrize_decorator(framework)
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_parametrize").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            let expected_parametrize_args = [
                HashMap::from([(String::from("arg1"), 1), (String::from("arg2"), 4)]),
                HashMap::from([(String::from("arg1"), 2), (String::from("arg2"), 5)]),
                HashMap::from([(String::from("arg1"), 3), (String::from("arg2"), 6)]),
            ];

            for (i, parametrize_arg) in tags.parametrize_args().iter().enumerate() {
                for (key, value) in parametrize_arg {
                    assert_eq!(
                        value.extract::<i32>(py).unwrap(),
                        expected_parametrize_args[i][&key.to_string()]
                    );
                }
            }
        });
    }

    #[rstest]
    fn test_parametrize_args_multiple_tags(#[values("karva", "pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r#"
import {}

{}("arg1", [1, 2, 3])
{}("arg2", [4, 5, 6])
def test_parametrize(arg1):
    pass
                "#,
                framework,
                get_parametrize_decorator(framework),
                get_parametrize_decorator(framework)
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_parametrize").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            let expected_parametrize_args = [
                HashMap::from([(String::from("arg1"), 1), (String::from("arg2"), 4)]),
                HashMap::from([(String::from("arg1"), 2), (String::from("arg2"), 4)]),
                HashMap::from([(String::from("arg1"), 3), (String::from("arg2"), 4)]),
                HashMap::from([(String::from("arg1"), 1), (String::from("arg2"), 5)]),
                HashMap::from([(String::from("arg1"), 2), (String::from("arg2"), 5)]),
                HashMap::from([(String::from("arg1"), 3), (String::from("arg2"), 5)]),
                HashMap::from([(String::from("arg1"), 1), (String::from("arg2"), 6)]),
                HashMap::from([(String::from("arg1"), 2), (String::from("arg2"), 6)]),
                HashMap::from([(String::from("arg1"), 3), (String::from("arg2"), 6)]),
            ];

            for (i, parametrize_arg) in tags.parametrize_args().iter().enumerate() {
                for (key, value) in parametrize_arg {
                    assert_eq!(
                        value.extract::<i32>(py).unwrap(),
                        expected_parametrize_args[i][&key.to_string()]
                    );
                }
            }
        });
    }

    #[rstest]
    fn test_use_fixtures_names_single(#[values("karva", "pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r#"
import {}

{}("my_fixture")
def test_function():
    pass
                "#,
                framework,
                get_usefixtures_decorator(framework)
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_function").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            let fixture_names = tags.required_fixtures_names();
            assert_eq!(fixture_names, vec!["my_fixture"]);
        });
    }

    #[rstest]
    fn test_use_fixtures_names_multiple(#[values("karva", "pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r#"
import {}

{}("fixture1", "fixture2", "fixture3")
def test_function():
    pass
                "#,
                framework,
                get_usefixtures_decorator(framework)
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_function").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            let fixture_names = tags.required_fixtures_names();
            assert_eq!(fixture_names, vec!["fixture1", "fixture2", "fixture3"]);
        });
    }

    #[rstest]
    fn test_use_fixtures_names_multiple_tags(#[values("karva", "pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r#"
import {}

{}("fixture1", "fixture2")
{}("fixture3")
def test_function():
    pass
                "#,
                framework,
                get_usefixtures_decorator(framework),
                get_usefixtures_decorator(framework)
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_function").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            let fixture_names: HashSet<_> = tags.required_fixtures_names().into_iter().collect();
            let expected: HashSet<_> = ["fixture1", "fixture2", "fixture3"]
                .iter()
                .copied()
                .map(String::from)
                .collect();
            assert_eq!(fixture_names, expected);
        });
    }

    #[rstest]
    fn test_empty_parametrize_values(#[values("karva", "pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r#"
import {}

{}("arg1", [])
def test_parametrize(arg1):
    pass
                "#,
                framework,
                get_parametrize_decorator(framework)
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_parametrize").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            let parametrize_args = tags.parametrize_args();
            assert_eq!(parametrize_args.len(), 0);
        });
    }

    #[rstest]
    fn test_mixed_parametrize_and_fixtures(#[values("karva", "pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r#"
import {}

{}("arg1", [1, 2])
{}("my_fixture")
def test_function(arg1):
    pass
                "#,
                framework,
                get_parametrize_decorator(framework),
                get_usefixtures_decorator(framework)
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_function").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            let parametrize_args = tags.parametrize_args();
            assert_eq!(parametrize_args.len(), 2);

            let fixture_names = tags.required_fixtures_names();
            assert_eq!(fixture_names, vec!["my_fixture"]);
        });
    }

    #[rstest]
    fn test_complex_parametrize_data_types(#[values("karva", "pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r#"
import {}

{}("arg1", ["string", 42, True, None])
def test_parametrize(arg1):
    pass
                "#,
                framework,
                get_parametrize_decorator(framework)
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_parametrize").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            let parametrize_args = tags.parametrize_args();
            assert_eq!(parametrize_args.len(), 4);

            assert_eq!(
                parametrize_args[0]["arg1"].extract::<String>(py).unwrap(),
                "string"
            );
            assert_eq!(parametrize_args[1]["arg1"].extract::<i32>(py).unwrap(), 42);
            assert!(parametrize_args[2]["arg1"].extract::<bool>(py).unwrap());
            assert!(parametrize_args[3]["arg1"].is_none(py));
        });
    }

    #[rstest]
    fn test_no_decorators(#[values("karva", "pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r"
import {framework}

def test_function():
    pass
                ",
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_function").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            assert!(tags.inner.is_empty());
        });
    }

    #[rstest]
    fn test_single_arg_tuple_parametrize(#[values("karva", "pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r#"
import {}

{}(("arg1",), [(1,), (2,), (3,)])
def test_parametrize(arg1):
    pass
                "#,
                framework,
                get_parametrize_decorator(framework)
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_parametrize").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            let parametrize_args = tags.parametrize_args();
            assert_eq!(parametrize_args.len(), 3);

            for (i, expected_val) in [1, 2, 3].iter().enumerate() {
                assert_eq!(
                    parametrize_args[i]["arg1"].extract::<i32>(py).unwrap(),
                    *expected_val
                );
            }
        });
    }

    #[rstest]
    fn test_skip_mark_with_reason_kwarg(#[values("pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r#"
import {}

{}(reason="Not implemented yet")
def test_skipped():
    pass
                "#,
                framework,
                get_skip_decorator(framework)
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_skipped").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            assert_eq!(tags.inner.len(), 1);
            if let Tag::Skip(skip_tag) = &tags.inner[0] {
                assert_eq!(skip_tag.reason(), Some("Not implemented yet"));
            } else {
                panic!("Expected Skip tag");
            }
        });
    }

    #[rstest]
    fn test_skip_mark_with_positional_reason(#[values("pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r#"
import {}

{}("some reason")
def test_skipped():
    pass
                "#,
                framework,
                get_skip_decorator(framework)
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_skipped").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            assert_eq!(tags.inner.len(), 1);
            if let Tag::Skip(skip_tag) = &tags.inner[0] {
                assert_eq!(skip_tag.reason(), Some("some reason"));
            } else {
                panic!("Expected Skip tag");
            }
        });
    }

    #[rstest]
    fn test_skip_mark_without_reason(#[values("pytest")] framework: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            let code = format!(
                r"
import {}

{}
def test_skipped():
    pass
                ",
                framework,
                get_skip_decorator(framework)
            );

            Python::run(py, &CString::new(code).unwrap(), None, Some(&locals)).unwrap();

            let test_function = locals.get_item("test_skipped").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None);

            assert_eq!(tags.inner.len(), 1);
            if let Tag::Skip(skip_tag) = &tags.inner[0] {
                assert_eq!(skip_tag.reason(), None);
            } else {
                panic!("Expected Skip tag");
            }
        });
    }
}
