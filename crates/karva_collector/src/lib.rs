//! Fast, syntax-only collection of Python test and fixture definitions.

use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;
use ruff_python_ast::{Expr, PythonVersion, Stmt};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
use ruff_text_size::{Ranged, TextRange, TextSize};
use thiserror::Error;

use karva_python_semantic::ModulePath;
use karva_python_semantic::is_fixture_function;

mod models;
mod parametrize;

pub use models::{CollectedDoctest, CollectedModule, CollectedPackage, DoctestTarget, ModuleType};
pub use parametrize::count_parametrize_cases;

#[derive(Debug, Error)]
/// Failure to load source required for syntax collection.
pub enum CollectionError {
    /// Python source could not be read from disk.
    #[error("failed to read Python source file `{path}`: {source}")]
    ReadSource {
        /// Source path that could not be read.
        path: Utf8PathBuf,

        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },
}

/// Settings that control how test files are collected and parsed.
pub struct CollectionSettings<'a> {
    /// The Python version to use when parsing source files.
    pub python_version: PythonVersion,

    /// The prefix used to identify test functions (e.g., `"test_"`).
    pub test_function_prefix: &'a str,

    /// Whether to respect `.gitignore` and similar ignore files during file discovery.
    pub respect_ignore_files: bool,

    /// Whether to collect fixture function definitions in addition to test functions.
    pub collect_fixtures: bool,

    /// Whether to collect docstrings containing doctest examples.
    pub collect_doctests: bool,
}

/// Collects tests and fixtures from a Python file.
///
/// If `function_names` is empty, all matching tests are collected. Otherwise, only tests whose
/// function name or doctest selector appears in the list are collected.
/// Fixtures are always collected regardless of the filter.
pub fn collect_file(
    path: &Utf8PathBuf,
    cwd: &Utf8Path,
    settings: &CollectionSettings,
    function_names: &[String],
) -> Result<Option<CollectedModule>, CollectionError> {
    let Some(module_path) = ModulePath::new(path, cwd) else {
        return Ok(None);
    };

    let source_text = fs::read_to_string(path).map_err(|source| CollectionError::ReadSource {
        path: path.clone(),
        source,
    })?;

    let module_type: ModuleType = path.into();

    let parse_options =
        ParseOptions::from(Mode::Module).with_target_version(settings.python_version);

    let Some(parsed) = parse_unchecked(&source_text, parse_options).try_into_module() else {
        return Ok(None);
    };

    let module_body = parsed.into_suite();
    let function_defs = module_body
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::FunctionDef(function_def) => Some(function_def.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut collected_module = CollectedModule::new(
        module_path,
        module_type,
        module_body.into_boxed_slice(),
        source_text,
    );

    if settings.collect_doctests && module_type == ModuleType::Test {
        for doctest in
            collect_doctests(&collected_module.module_body, &collected_module.source_text)
        {
            if function_names.is_empty() || function_names.iter().any(|name| name == &doctest.name)
            {
                collected_module.add_doctest(doctest);
            }
        }
    }

    for function_def in function_defs {
        if settings.collect_fixtures && is_fixture_function(&function_def) {
            collected_module.add_fixture_function_def(function_def);
            continue;
        }

        if is_test_function_to_collect(
            &function_def.name,
            function_names,
            settings.test_function_prefix,
        ) {
            collected_module.add_test_function_def(function_def);
        }
    }

    Ok(Some(collected_module))
}

fn collect_doctests(module_body: &[Stmt], source_text: &str) -> Vec<CollectedDoctest> {
    let mut doctests = Vec::new();
    collect_body_doctest(
        module_body,
        DoctestTarget::Module,
        source_text,
        &mut doctests,
    );

    for statement in module_body {
        collect_object_doctests(statement, "", source_text, &mut doctests);
    }

    doctests
}

fn collect_object_doctests(
    statement: &Stmt,
    parent: &str,
    source_text: &str,
    doctests: &mut Vec<CollectedDoctest>,
) {
    let (name, body, recurse) = match statement {
        Stmt::FunctionDef(function) => (function.name.as_str(), function.body.as_slice(), false),
        Stmt::ClassDef(class) => (class.name.as_str(), class.body.as_slice(), true),
        _ => return,
    };
    let object_name = if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}.{name}")
    };
    remove_redefined_doctests(doctests, &object_name);
    collect_body_doctest(
        body,
        DoctestTarget::Object(object_name.clone()),
        source_text,
        doctests,
    );

    if recurse {
        for statement in body {
            collect_object_doctests(statement, &object_name, source_text, doctests);
        }
    }
}

fn remove_redefined_doctests(doctests: &mut Vec<CollectedDoctest>, object_name: &str) {
    doctests.retain(|doctest| match &doctest.target {
        DoctestTarget::Module => true,
        DoctestTarget::Object(existing) => {
            existing != object_name
                && !existing
                    .strip_prefix(object_name)
                    .is_some_and(|suffix| suffix.starts_with('.'))
        }
    });
}

fn collect_body_doctest(
    body: &[Stmt],
    target: DoctestTarget,
    source_text: &str,
    doctests: &mut Vec<CollectedDoctest>,
) {
    let Some(Stmt::Expr(docstring)) = body.first() else {
        return;
    };
    let Expr::StringLiteral(value) = docstring.value.as_ref() else {
        return;
    };
    if !value.value.to_str().lines().any(is_doctest_prompt) {
        return;
    }

    let range = prompt_range(source_text, value.range()).unwrap_or_else(|| value.range());
    let name = match &target {
        DoctestTarget::Module => "doctest:@module".to_string(),
        DoctestTarget::Object(object_name) => format!("doctest:{object_name}"),
    };
    doctests.push(CollectedDoctest {
        target,
        name,
        range,
    });
}

fn is_doctest_prompt(line: &str) -> bool {
    let Some(rest) = line.trim_start().strip_prefix(">>>") else {
        return false;
    };
    let source = rest.trim_start();
    rest.starts_with(char::is_whitespace) && !source.is_empty() && !source.starts_with('#')
}

fn prompt_range(source_text: &str, range: TextRange) -> Option<TextRange> {
    let source = source_text.get(usize::from(range.start())..usize::from(range.end()))?;
    let mut line_offset = 0;
    let mut prompt_offset = None;
    for line in source.split_inclusive('\n') {
        if is_doctest_prompt(line)
            && let Some(prompt) = line.find(">>>")
        {
            prompt_offset = Some(line_offset + prompt);
            break;
        }
        line_offset += line.len();
    }
    let offset = prompt_offset?;
    let offset = TextSize::try_from(offset).ok()?;
    Some(TextRange::at(range.start() + offset, TextSize::new(3)))
}

/// Returns `true` if a function should be collected as a test.
///
/// When `explicit_names` is empty, any function whose name starts with
/// `prefix` is considered a test. When `explicit_names` is provided,
/// only functions whose name appears in the list are collected.
fn is_test_function_to_collect(name: &str, explicit_names: &[String], prefix: &str) -> bool {
    if explicit_names.is_empty() {
        name.starts_with(prefix)
    } else {
        explicit_names.iter().any(|n| n == name)
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use ruff_python_ast::PythonVersion;

    use super::*;

    fn settings() -> CollectionSettings<'static> {
        CollectionSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test_",
            respect_ignore_files: true,
            collect_fixtures: false,
            collect_doctests: false,
        }
    }

    #[test]
    fn collect_file_reports_read_errors() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let cwd = Utf8Path::from_path(temp_dir.path()).expect("temp dir should be UTF-8");
        let path = cwd.join("test_unreadable.py");
        std::fs::create_dir(&path).expect("create directory at Python file path");

        let error = collect_file(&path, cwd, &settings(), &[]).expect_err("read should fail");

        assert!(matches!(
            error,
            CollectionError::ReadSource { path: error_path, .. } if error_path == path
        ));
    }

    #[test]
    fn collect_file_collects_prefixed_tests() {
        let (_temp_dir, root, path) = python_file(
            "test_sample.py",
            "def helper(): pass\n\
             def test_first(): pass\n\
             def test_second(): pass\n",
        );

        let module = collect_file(&path, &root, &settings(), &[])
            .expect("collect file")
            .expect("module should collect");

        assert_eq!(
            function_names(&module.test_function_defs),
            ["test_first", "test_second"]
        );
        assert!(module.fixture_function_defs.is_empty());
    }

    #[test]
    fn collect_file_collects_explicit_function_names() {
        let (_temp_dir, root, path) = python_file(
            "test_sample.py",
            "def helper(): pass\n\
             def test_first(): pass\n\
             def test_second(): pass\n",
        );

        let module = collect_file(&path, &root, &settings(), &["helper".to_string()])
            .expect("collect file")
            .expect("module should collect");

        assert_eq!(function_names(&module.test_function_defs), ["helper"]);
    }

    #[test]
    fn collect_file_collects_fixtures_when_enabled() {
        let (_temp_dir, root, path) = python_file(
            "test_sample.py",
            "from karva import fixture\n\
             @fixture\n\
             def db(): pass\n\
             def test_uses_db(): pass\n",
        );
        let settings = CollectionSettings {
            collect_fixtures: true,
            ..settings()
        };

        let module = collect_file(&path, &root, &settings, &[])
            .expect("collect file")
            .expect("module should collect");

        assert_eq!(function_names(&module.fixture_function_defs), ["db"]);
        assert_eq!(function_names(&module.test_function_defs), ["test_uses_db"]);
    }

    #[test]
    fn collect_file_collects_module_function_class_and_method_doctests() {
        let (_temp_dir, root, path) = python_file(
            "sample.py",
            r#"
"""Module docs.

>>> 1 + 1
2
"""
def function():
    """Function docs.

    >>> 2 + 2
    4
    """
class Thing:
    """Class docs.

    >>> 3 + 3
    6
    """
    def method(self):
        """Method docs.

        >>> 4 + 4
        8
        """
def ordinary():
    """Documentation without an example."""
"#,
        );
        let settings = CollectionSettings {
            collect_doctests: true,
            ..settings()
        };

        let module = collect_file(&path, &root, &settings, &[])
            .expect("collect file")
            .expect("module should collect");

        let names: Vec<_> = module
            .doctests
            .iter()
            .map(|doctest| doctest.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "doctest:@module",
                "doctest:function",
                "doctest:Thing",
                "doctest:Thing.method"
            ]
        );
    }

    #[test]
    fn collect_file_ignores_doctests_when_disabled() {
        let (_temp_dir, root, path) = python_file(
            "sample.py",
            r#"
"""
>>> 1 + 1
2
"""
"#,
        );

        let module = collect_file(&path, &root, &settings(), &[])
            .expect("collect file")
            .expect("module should collect");

        assert!(module.doctests.is_empty());
    }

    #[test]
    fn collect_file_filters_doctests_by_selector() {
        let (_temp_dir, root, path) = python_file(
            "sample.py",
            r#"
"""
>>> 1 + 1
2
"""
class Thing:
    """
    >>> 2 + 2
    4
    """
"#,
        );
        let settings = CollectionSettings {
            collect_doctests: true,
            ..settings()
        };

        let module = collect_file(&path, &root, &settings, &["doctest:Thing".to_string()])
            .expect("collect file")
            .expect("module should collect");

        assert_eq!(module.doctests.len(), 1);
        assert_eq!(
            module.doctests[0].target,
            DoctestTarget::Object("Thing".to_string())
        );
    }

    #[test]
    fn doctest_prompt_requires_executable_source() {
        assert!(is_doctest_prompt(">>> value"));
        assert!(is_doctest_prompt("    >>>\tvalue"));
        assert!(!is_doctest_prompt(">>>"));
        assert!(!is_doctest_prompt(">>>   "));
        assert!(!is_doctest_prompt(">>> # comment"));
        assert!(!is_doctest_prompt(">>>value"));

        let source = "\"\"\"Mention >>> inline.\n>>> value\n\"\"\"";
        let range = TextRange::new(
            TextSize::default(),
            TextSize::try_from(source.len()).expect("source length should fit"),
        );
        let prompt = prompt_range(source, range).expect("find executable prompt");
        assert_eq!(
            usize::from(prompt.start()),
            source.find("\n>>>").expect("find prompt line") + 1
        );
    }

    #[test]
    fn collect_file_uses_the_final_object_binding() {
        let (_temp_dir, root, path) = python_file(
            "sample.py",
            r#"
def documented():
    """
    >>> 1
    1
    """

def documented():
    """
    >>> 2
    2
    """

class Thing:
    def stale(self):
        """
        >>> 3
        3
        """

class Thing:
    """
    >>> 4
    4
    """
"#,
        );
        let settings = CollectionSettings {
            collect_doctests: true,
            ..settings()
        };

        let module = collect_file(&path, &root, &settings, &[])
            .expect("collect file")
            .expect("module should collect");

        let names: Vec<_> = module
            .doctests
            .iter()
            .map(|doctest| doctest.name.as_str())
            .collect();
        assert_eq!(names, ["doctest:documented", "doctest:Thing"]);
    }

    #[test]
    fn collect_file_skips_paths_outside_cwd() {
        let (_temp_dir, _root, path) = python_file("test_sample.py", "def test_sample(): pass\n");
        let outside_dir = tempfile::tempdir().expect("create outside temp dir");
        let cwd = Utf8PathBuf::from_path_buf(outside_dir.path().to_path_buf())
            .expect("outside temp path should be UTF-8");

        let module = collect_file(&path, &cwd, &settings(), &[]).expect("collect file");

        assert!(module.is_none());
    }

    fn python_file(name: &str, source: &str) -> (tempfile::TempDir, Utf8PathBuf, Utf8PathBuf) {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf())
            .expect("temp path should be UTF-8");
        let path = root.join(name);
        std::fs::write(&path, source).expect("write Python file");

        (temp_dir, root, path)
    }

    fn function_names(functions: &[ruff_python_ast::StmtFunctionDef]) -> Vec<&str> {
        functions
            .iter()
            .map(|function| function.name.as_str())
            .collect()
    }
}
