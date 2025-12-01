use std::{ops::Deref, sync::Arc};

use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::extensions::tags::python::{PyTag, PyTags, PyTestFunction};

pub mod expect_fail;
pub mod parametrize;
pub mod python;
pub mod skip;
mod use_fixtures;

use expect_fail::ExpectFailTag;
pub use parametrize::Parametrization;
use parametrize::{ParametrizationArgs, ParametrizeTag};
use skip::SkipTag;
use use_fixtures::UseFixturesTag;

/// Represents a decorator function in Python that can be used to extend the functionality of a test.
#[derive(Debug, Clone)]
pub enum Tag {
    Parametrize(ParametrizeTag),
    UseFixtures(UseFixturesTag),
    Skip(SkipTag),
    ExpectFail(ExpectFailTag),
}

impl Tag {
    /// Converts a Pytest mark into an Karva Tag.
    ///
    /// This is used to allow Pytest marks to be used as Karva tags.
    fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let name = py_mark.getattr("name").ok()?.extract::<String>().ok()?;
        match name.as_str() {
            "parametrize" => ParametrizeTag::try_from_pytest_mark(py_mark).map(Self::Parametrize),
            "usefixtures" => UseFixturesTag::try_from_pytest_mark(py_mark).map(Self::UseFixtures),
            "skip" | "skipif" => SkipTag::try_from_pytest_mark(py_mark).map(Self::Skip),
            "xfail" => ExpectFailTag::try_from_pytest_mark(py_mark).map(Self::ExpectFail),
            _ => None,
        }
    }

    /// Try to create a tag object from a Python object.
    ///
    /// We first check if the object is a `PyTag` or `PyTags`.
    /// If not, we try to call it to see if it returns a `PyTag` or `PyTags`.
    pub(crate) fn try_from_py_any(py: Python, py_any: &Py<PyAny>) -> Option<Self> {
        if let Ok(tag) = py_any.cast_bound::<PyTag>(py) {
            return Some(Self::from_karva_tag(tag.borrow()));
        } else if let Ok(tag) = py_any.cast_bound::<PyTags>(py)
            && let Some(tag) = tag.borrow().inner.first()
        {
            return Some(Self::from_karva_tag(tag));
        } else if let Ok(tag) = py_any.call0(py) {
            if let Ok(tag) = tag.cast_bound::<PyTag>(py) {
                return Some(Self::from_karva_tag(tag.borrow()));
            }
            if let Ok(tag) = tag.cast_bound::<PyTags>(py)
                && let Some(tag) = tag.borrow().inner.first()
            {
                return Some(Self::from_karva_tag(tag));
            }
        }

        None
    }

    /// Converts a Karva Python tag into our internal representation.
    pub(crate) fn from_karva_tag<T>(py_tag: T) -> Self
    where
        T: Deref<Target = PyTag>,
    {
        match &*py_tag {
            PyTag::Parametrize {
                arg_names,
                arg_values,
            } => Self::Parametrize(ParametrizeTag::from_karva(
                arg_names.clone(),
                arg_values.clone(),
            )),
            PyTag::UseFixtures { fixture_names } => {
                Self::UseFixtures(UseFixturesTag::new(fixture_names.clone()))
            }
            PyTag::Skip { conditions, reason } => {
                Self::Skip(SkipTag::new(conditions.clone(), reason.clone()))
            }
            PyTag::ExpectFail { conditions, reason } => {
                Self::ExpectFail(ExpectFailTag::new(conditions.clone(), reason.clone()))
            }
        }
    }
}

/// Represents a collection of tags associated with a test function.
///
/// This means we can collect tags and use them all for the same function.
#[derive(Debug, Clone)]
pub struct Tags {
    inner: Arc<Vec<Tag>>,
}

impl Tags {
    pub(crate) fn new(tags: Vec<Tag>) -> Self {
        Self {
            inner: Arc::new(tags),
        }
    }

    pub(crate) fn extend(&mut self, other: &Self) {
        let self_vec = Arc::make_mut(&mut self.inner);
        self_vec.extend(other.inner.iter().cloned());
    }

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
                tags.push(Tag::from_karva_tag(tag));
            }
            return Self::new(tags);
        } else if let Ok(wrapped) = py_function.getattr(py, "__wrapped__") {
            if let Ok(py_wrapped_function) = wrapped.extract::<Py<PyTestFunction>>(py) {
                let mut tags = Vec::new();
                for tag in &py_wrapped_function.borrow(py).tags.inner {
                    tags.push(Tag::from_karva_tag(tag));
                }
                return Self::new(tags);
            }
        }

        if let Ok(marks) = py_function.getattr(py, "pytestmark")
            && let Some(tags) = Self::from_pytest_marks(py, &marks)
        {
            return tags;
        }

        Self::default()
    }

    pub(crate) fn from_pytest_marks(py: Python<'_>, marks: &Py<PyAny>) -> Option<Self> {
        let mut tags = Vec::new();
        if let Ok(marks_list) = marks.extract::<Vec<Bound<'_, PyAny>>>(py) {
            for mark in marks_list {
                if let Some(tag) = Tag::try_from_pytest_mark(&mark) {
                    tags.push(tag);
                }
            }
        } else {
            return None;
        }
        Some(Self {
            inner: Arc::new(tags),
        })
    }

    /// Return all parametrizations
    ///
    /// This function ensures that if we have multiple parametrize tags, we combine them together.
    pub(crate) fn parametrize_args(&self) -> Vec<ParametrizationArgs> {
        let mut param_args: Vec<ParametrizationArgs> = vec![ParametrizationArgs::default()];

        for tag in self.inner.iter() {
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
    pub(crate) fn required_fixtures_names(&self) -> Vec<String> {
        let mut fixture_names = Vec::new();
        for tag in self.inner.iter() {
            if let Tag::UseFixtures(use_fixtures_tag) = tag {
                fixture_names.extend_from_slice(use_fixtures_tag.fixture_names());
            }
        }
        fixture_names
    }

    /// Returns true if any skip tag should be skipped.
    pub(crate) fn should_skip(&self) -> (bool, Option<String>) {
        for tag in self.inner.iter() {
            if let Tag::Skip(skip_tag) = tag {
                if skip_tag.should_skip() {
                    return (true, skip_tag.reason());
                }
            }
        }
        (false, None)
    }

    /// Return the `ExpectFailTag` if it exists.
    pub(crate) fn expect_fail_tag(&self) -> Option<ExpectFailTag> {
        for tag in self.inner.iter() {
            if let Tag::ExpectFail(expect_fail_tag) = tag {
                return Some(expect_fail_tag.clone());
            }
        }
        None
    }
}

impl Default for Tags {
    fn default() -> Self {
        Self {
            inner: Arc::new(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        ffi::CString,
    };

    use pyo3::{prelude::*, types::PyDict};
    use rstest::rstest;

    use super::*;
    use crate::utils::attach;

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

    fn get_tags(py: Python<'_>, source: &str) -> Tags {
        let locals = PyDict::new(py);
        Python::run(py, &CString::new(source).unwrap(), None, Some(&locals)).unwrap();

        let test_function = locals.get_item("test_function").unwrap().unwrap();
        let test_function = test_function.as_unbound();
        Tags::from_py_any(py, test_function, None)
    }

    #[rstest]
    fn test_function_args_single_arg(#[values("karva", "pytest")] framework: &str) {
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
                    r#"
import {}

{}("arg1", [1, 2, 3])
def test_function(arg1):
    pass
                "#,
                    framework,
                    get_parametrize_decorator(framework)
                ),
            );

            let expected_parametrize_args = [
                HashMap::from([(String::from("arg1"), 1)]),
                HashMap::from([(String::from("arg1"), 2)]),
                HashMap::from([(String::from("arg1"), 3)]),
            ];

            for (i, parametrize_arg) in tags.parametrize_args().iter().enumerate() {
                for (key, value) in parametrize_arg.values() {
                    assert_eq!(
                        value.extract::<i32>(py).unwrap(),
                        expected_parametrize_args[i][key]
                    );
                }
            }
        });
    }

    #[rstest]
    fn test_function_args_two_args(#[values("karva", "pytest")] framework: &str) {
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
                    r#"
import {}

{}(("arg1", "arg2"), [(1, 4), (2, 5), (3, 6)])
def test_function(arg1, arg2):
    pass
                "#,
                    framework,
                    get_parametrize_decorator(framework)
                ),
            );

            let expected_parametrize_args = [
                HashMap::from([(String::from("arg1"), 1), (String::from("arg2"), 4)]),
                HashMap::from([(String::from("arg1"), 2), (String::from("arg2"), 5)]),
                HashMap::from([(String::from("arg1"), 3), (String::from("arg2"), 6)]),
            ];

            for (i, parametrize_arg) in tags.parametrize_args().iter().enumerate() {
                for (key, value) in parametrize_arg.values() {
                    assert_eq!(
                        value.extract::<i32>(py).unwrap(),
                        expected_parametrize_args[i][key]
                    );
                }
            }
        });
    }

    #[rstest]
    fn test_function_args_multiple_tags(#[values("karva", "pytest")] framework: &str) {
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
                    r#"
import {}

{}("arg1", [1, 2, 3])
{}("arg2", [4, 5, 6])
def test_function(arg1):
    pass
                "#,
                    framework,
                    get_parametrize_decorator(framework),
                    get_parametrize_decorator(framework)
                ),
            );

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
                for (key, value) in parametrize_arg.values() {
                    assert_eq!(
                        value.extract::<i32>(py).unwrap(),
                        expected_parametrize_args[i][key]
                    );
                }
            }
        });
    }

    #[rstest]
    fn test_use_fixtures_names_single(#[values("karva", "pytest")] framework: &str) {
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
                    r#"
import {}

{}("my_fixture")
def test_function():
    pass
                "#,
                    framework,
                    get_usefixtures_decorator(framework)
                ),
            );

            let fixture_names = tags.required_fixtures_names();
            assert_eq!(fixture_names, vec!["my_fixture"]);
        });
    }

    #[rstest]
    fn test_use_fixtures_names_multiple(#[values("karva", "pytest")] framework: &str) {
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
                    r#"
import {}

{}("fixture1", "fixture2", "fixture3")
def test_function():
    pass
                "#,
                    framework,
                    get_usefixtures_decorator(framework)
                ),
            );

            let fixture_names = tags.required_fixtures_names();
            assert_eq!(fixture_names, vec!["fixture1", "fixture2", "fixture3"]);
        });
    }

    #[rstest]
    fn test_use_fixtures_names_multiple_tags(#[values("karva", "pytest")] framework: &str) {
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
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
                ),
            );

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
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
                    r#"
import {}

{}("arg1", [])
def test_function(arg1):
    pass
                "#,
                    framework,
                    get_parametrize_decorator(framework)
                ),
            );

            let parametrize_args = tags.parametrize_args();
            assert_eq!(parametrize_args.len(), 0);
        });
    }

    #[rstest]
    fn test_mixed_parametrize_and_fixtures(#[values("karva", "pytest")] framework: &str) {
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
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
                ),
            );

            let parametrize_args = tags.parametrize_args();
            assert_eq!(parametrize_args.len(), 2);

            let fixture_names = tags.required_fixtures_names();
            assert_eq!(fixture_names, vec!["my_fixture"]);
        });
    }

    #[rstest]
    fn test_complex_parametrize_data_types(#[values("karva", "pytest")] framework: &str) {
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
                    r#"
import {}

{}("arg1", ["string", 42, True, None])
def test_function(arg1):
    pass
                "#,
                    framework,
                    get_parametrize_decorator(framework)
                ),
            );

            let parametrize_args = tags.parametrize_args();
            assert_eq!(parametrize_args.len(), 4);

            assert_eq!(
                parametrize_args[0].values()["arg1"]
                    .extract::<String>(py)
                    .unwrap(),
                "string"
            );
            assert_eq!(
                parametrize_args[1].values()["arg1"]
                    .extract::<i32>(py)
                    .unwrap(),
                42
            );
            assert!(
                parametrize_args[2].values()["arg1"]
                    .extract::<bool>(py)
                    .unwrap()
            );
            assert!(parametrize_args[3].values()["arg1"].is_none(py));
        });
    }

    #[rstest]
    fn test_no_decorators(#[values("karva", "pytest")] framework: &str) {
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
                    r"
import {framework}

def test_function():
    pass
                ",
                ),
            );

            assert!(tags.inner.is_empty());
        });
    }

    #[rstest]
    fn test_single_arg_tuple_parametrize(#[values("karva", "pytest")] framework: &str) {
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
                    r#"
import {}

{}(("arg1",), [(1,), (2,), (3,)])
def test_function(arg1):
    pass
                "#,
                    framework,
                    get_parametrize_decorator(framework)
                ),
            );

            let parametrize_args = tags.parametrize_args();
            assert_eq!(parametrize_args.len(), 3);

            for (i, expected_val) in [1, 2, 3].iter().enumerate() {
                assert_eq!(
                    parametrize_args[i].values()["arg1"]
                        .extract::<i32>(py)
                        .unwrap(),
                    *expected_val
                );
            }
        });
    }

    #[rstest]
    fn test_skip_mark_with_reason_kwarg(#[values("pytest")] framework: &str) {
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
                    r#"
import {}

{}(reason="Not implemented yet")
def test_function():
    pass
                "#,
                    framework,
                    get_skip_decorator(framework)
                ),
            );

            assert_eq!(tags.inner.len(), 1);
            if let Tag::Skip(skip_tag) = &tags.inner[0] {
                assert_eq!(skip_tag.reason(), Some("Not implemented yet".to_string()));
            } else {
                panic!("Expected Skip tag");
            }
        });
    }

    #[rstest]
    fn test_skip_mark_with_positional_reason(#[values("pytest")] framework: &str) {
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
                    r#"
import {}

{}("some reason")
def test_function():
    pass
                "#,
                    framework,
                    get_skip_decorator(framework)
                ),
            );

            assert_eq!(tags.inner.len(), 1);
            if let Tag::Skip(skip_tag) = &tags.inner[0] {
                assert_eq!(skip_tag.reason(), Some("some reason".to_string()));
            } else {
                panic!("Expected Skip tag");
            }
        });
    }

    #[rstest]
    fn test_skip_mark_without_reason(#[values("pytest")] framework: &str) {
        attach(|py| {
            let tags = get_tags(
                py,
                &format!(
                    r"
import {}

{}
def test_function():
    pass
                ",
                    framework,
                    get_skip_decorator(framework)
                ),
            );

            assert_eq!(tags.inner.len(), 1);
            if let Tag::Skip(skip_tag) = &tags.inner[0] {
                assert_eq!(skip_tag.reason(), None);
            } else {
                panic!("Expected Skip tag");
            }
        });
    }
}
