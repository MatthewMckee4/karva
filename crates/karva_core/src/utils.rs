use std::collections::HashMap;

use camino::Utf8Path;
use karva_project::project::{Project, ProjectOptions};
use pyo3::{PyResult, Python, prelude::*, types::PyAnyMethods};
use ruff_python_ast::{PythonVersion, StmtFunctionDef};

use crate::discovery::DiscoveredModule;

/// Retrieves the current Python interpreter version.
///
/// This function queries the embedded Python interpreter to determine
/// the major and minor version numbers, which are used for AST parsing
/// compatibility and feature detection.
pub fn current_python_version() -> PythonVersion {
    PythonVersion::from(Python::attach(|py| {
        let version_info = py.version_info();
        (version_info.major, version_info.minor)
    }))
}

/// Adds a directory path to Python's sys.path at the specified index.
pub(crate) fn add_to_sys_path(py: Python<'_>, path: &Utf8Path, index: isize) -> PyResult<()> {
    let sys_module = py.import("sys")?;
    let sys_path = sys_module.getattr("path")?;
    sys_path.call_method1("insert", (index, path.to_string()))?;
    Ok(())
}

/// Redirects Python's stdout and stderr to /dev/null if output is disabled.
///
/// This function is used to suppress Python output during test execution
/// when the user hasn't requested to see it. It returns a handle to the
/// null file for later restoration.
fn redirect_python_output<'py>(
    py: Python<'py>,
    options: &ProjectOptions,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    if options.show_output() {
        Ok(None)
    } else {
        let sys = py.import("sys")?;
        let os = py.import("os")?;
        let builtins = py.import("builtins")?;
        let logging = py.import("logging")?;

        let devnull = os.getattr("devnull")?;
        let open_file_function = builtins.getattr("open")?;
        let null_file = open_file_function.call1((devnull, "w"))?;

        for output in ["stdout", "stderr"] {
            sys.setattr(output, null_file.clone())?;
        }

        logging.call_method1("disable", (logging.getattr("CRITICAL")?,))?;

        Ok(Some(null_file))
    }
}

/// Restores Python's stdout and stderr from the null file redirect.
///
/// This function cleans up the output redirection by closing the null file
/// handles and restoring normal output streams.
fn restore_python_output<'py>(py: Python<'py>, null_file: &Bound<'py, PyAny>) -> PyResult<()> {
    let sys = py.import("sys")?;
    let logging = py.import("logging")?;

    for output in ["stdout", "stderr"] {
        let current_output = sys.getattr(output)?;
        let close_method = current_output.getattr("close")?;
        close_method.call0()?;
        sys.setattr(output, null_file.clone())?;
    }

    logging.call_method1("disable", (logging.getattr("CRITICAL")?,))?;
    Ok(())
}

/// A wrapper around `Python::attach` so we can manage the stdout and stderr redirection.
pub(crate) fn attach<F, R>(project: &Project, f: F) -> R
where
    F: for<'py> FnOnce(Python<'py>) -> R,
{
    Python::attach(|py| {
        let null_file = redirect_python_output(py, project.options());
        let result = f(py);
        if let Ok(Some(null_file)) = null_file {
            let _ = restore_python_output(py, &null_file);
        }
        result
    })
}

/// Creates an iterator that yields each item with all items after it.
///
/// For example, given [session, package, module],
/// it yields: (module, [session, package]), (package, [session]), (session, []).
pub(crate) fn iter_with_ancestors<'a, T: ?Sized>(
    items: &[&'a T],
) -> impl Iterator<Item = (&'a T, Vec<&'a T>)> {
    let mut ancestors = items.to_vec();
    let mut current_index = items.len();

    std::iter::from_fn(move || {
        if current_index > 0 {
            current_index -= 1;
            let current_item = items[current_index];
            ancestors.truncate(current_index);
            Some((current_item, ancestors.clone()))
        } else {
            None
        }
    })
}

/// Creates a cartesian product by inserting new items into existing `HashMaps`.
///
/// For each `HashMap` in `existing` and each item in `new_items`, this creates a new
/// `HashMap` with the item inserted at `key`. The `create_value` function transforms
/// each item before insertion.
///
/// # Example
/// // existing = [{"a": 1}, {"b": 2}]
/// // `new_items` = [10, 20]
/// // key = "c"
/// // Result: [{"a": 1, "c": 10}, {"a": 1, "c": 20}, {"b": 2, "c": 10}, {"b": 2, "c": 20}]
///
/// This is primarily used for parametrized fixtures where all combinations of
/// fixture values need to be generated.
pub(crate) fn cartesian_insert<T, F>(
    existing: Vec<HashMap<String, T>>,
    new_items: &[T],
    key: &str,
    mut create_value: F,
) -> PyResult<Vec<HashMap<String, T>>>
where
    T: Clone,
    F: FnMut(&T) -> PyResult<T>,
{
    let mut result = Vec::new();

    for fixtures in existing {
        for item in new_items {
            let mut new_fixtures = fixtures.clone();
            let value = create_value(item)?;
            new_fixtures.insert(key.to_string(), value);
            result.push(new_fixtures);
        }
    }

    Ok(result)
}

pub(crate) fn function_definition_location(
    module: &DiscoveredModule,
    stmt_function_def: &StmtFunctionDef,
) -> String {
    let line_index = module.line_index();
    let source_text = module.source_text();
    let start = stmt_function_def.range.start();
    let line_number = line_index.line_column(start, source_text);
    format!("{}:{}", module.path(), line_number.line)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod python_version_tests {
        use super::*;

        #[test]
        fn test_current_python_version() {
            let version = current_python_version();
            assert!(version >= PythonVersion::from((3, 7)));
        }
    }

    mod utils_tests {
        use super::*;

        #[test]
        fn test_iter_with_ancestors() {
            let items = vec!["session", "package", "module"];
            let expected = vec![
                ("module", vec!["session", "package"]),
                ("package", vec!["session"]),
                ("session", vec![]),
            ];
            let result: Vec<(&str, Vec<&str>)> = iter_with_ancestors(&items).collect();
            assert_eq!(result, expected);
        }
    }

    mod cartesian_insert_tests {
        use super::*;

        #[test]
        fn test_cartesian_insert_basic() {
            let mut map1 = HashMap::new();
            map1.insert("a".to_string(), 1);

            let mut map2 = HashMap::new();
            map2.insert("b".to_string(), 2);

            let existing = vec![map1, map2];
            let new_items = vec![10, 20];

            let result = cartesian_insert(existing, &new_items, "c", |item| Ok(*item)).unwrap();

            assert_eq!(result.len(), 4);

            assert_eq!(result[0].get("a"), Some(&1));
            assert_eq!(result[0].get("c"), Some(&10));

            assert_eq!(result[1].get("a"), Some(&1));
            assert_eq!(result[1].get("c"), Some(&20));

            assert_eq!(result[2].get("b"), Some(&2));
            assert_eq!(result[2].get("c"), Some(&10));

            assert_eq!(result[3].get("b"), Some(&2));
            assert_eq!(result[3].get("c"), Some(&20));
        }

        #[test]
        fn test_cartesian_insert_single_existing() {
            let mut map = HashMap::new();
            map.insert("x".to_string(), "original");

            let existing = vec![map];
            let new_items = vec!["foo", "bar", "baz"];

            let result = cartesian_insert(existing, &new_items, "y", |item| Ok(*item)).unwrap();

            assert_eq!(result.len(), 3);
            assert_eq!(result[0].get("x"), Some(&"original"));
            assert_eq!(result[0].get("y"), Some(&"foo"));
            assert_eq!(result[1].get("y"), Some(&"bar"));
            assert_eq!(result[2].get("y"), Some(&"baz"));
        }

        #[test]
        fn test_cartesian_insert_empty_existing() {
            let existing: Vec<HashMap<String, i32>> = vec![];
            let new_items = vec![1, 2, 3];

            let result = cartesian_insert(existing, &new_items, "key", |item| Ok(*item)).unwrap();

            assert_eq!(result.len(), 0);
        }

        #[test]
        fn test_cartesian_insert_empty_new_items() {
            let mut map = HashMap::new();
            map.insert("a".to_string(), 1);

            let existing = vec![map];
            let new_items: Vec<i32> = vec![];

            let result = cartesian_insert(existing, &new_items, "b", |item| Ok(*item)).unwrap();

            assert_eq!(result.len(), 0);
        }

        #[test]
        fn test_cartesian_insert_with_transform() {
            let mut map = HashMap::new();
            map.insert("base".to_string(), 0);

            let existing = vec![map];
            let new_items = vec![1, 2, 3];

            let result =
                cartesian_insert(existing, &new_items, "doubled", |item| Ok(item * 2)).unwrap();

            assert_eq!(result.len(), 3);
            assert_eq!(result[0].get("doubled"), Some(&2));
            assert_eq!(result[1].get("doubled"), Some(&4));
            assert_eq!(result[2].get("doubled"), Some(&6));
        }

        #[test]
        fn test_cartesian_insert_preserves_original_maps() {
            let mut map1 = HashMap::new();
            map1.insert("a".to_string(), 1);
            map1.insert("b".to_string(), 2);

            let mut map2 = HashMap::new();
            map2.insert("x".to_string(), 10);

            let existing = vec![map1, map2];
            let new_items = vec![100];

            let result = cartesian_insert(existing, &new_items, "new", |item| Ok(*item)).unwrap();

            assert_eq!(result.len(), 2);

            assert_eq!(result[0].get("a"), Some(&1));
            assert_eq!(result[0].get("b"), Some(&2));
            assert_eq!(result[0].get("new"), Some(&100));

            assert_eq!(result[1].get("x"), Some(&10));
            assert_eq!(result[1].get("new"), Some(&100));
        }

        #[test]
        fn test_cartesian_insert_error_propagation() {
            let mut map = HashMap::new();
            map.insert("key".to_string(), 1);

            let existing = vec![map];
            let new_items = vec![1, 2, 3];

            let result = cartesian_insert(existing, &new_items, "value", |item| {
                if *item == 2 {
                    Err(pyo3::exceptions::PyValueError::new_err("Test error"))
                } else {
                    Ok(*item)
                }
            });

            assert!(result.is_err());
        }
    }
}
