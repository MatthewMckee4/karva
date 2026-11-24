use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize, Serializer};

use crate::module_name;

/// Stable function identity serialized as `<module>::<function>`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QualifiedFunctionName {
    function_name: String,
    module_path: ModulePath,
}

impl QualifiedFunctionName {
    /// Combines an unqualified Python name with its owning module.
    pub fn new(function_name: String, module_path: ModulePath) -> Self {
        Self {
            function_name,
            module_path,
        }
    }

    /// Return the unqualified function name.
    pub fn function_name(&self) -> &str {
        &self.function_name
    }

    /// Return the module path this function belongs to.
    pub fn module_path(&self) -> &ModulePath {
        &self.module_path
    }
}

impl std::fmt::Display for QualifiedFunctionName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}::{}",
            self.module_path.module_name(),
            self.function_name
        )
    }
}

impl Serialize for QualifiedFunctionName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

/// A worker case index, with its controller selector when a fixture adds dimensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParameterCase {
    Index(usize),
    Fixture([usize; 2]),
}

impl ParameterCase {
    /// Forms the cache identity shared by worker results and crash checkpoints.
    pub fn cache_key(self, function: impl std::fmt::Display) -> TestCacheKey {
        match self {
            Self::Index(index) => TestCacheKey(format!("{function}[{index}]")),
            Self::Fixture([group, index]) => TestCacheKey(format!("{function}[{group}:{index}]")),
        }
    }
}

/// Stable duration and last-failed identity for a function or parameter case.
#[derive(Clone, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TestCacheKey(String);

impl TestCacheKey {
    /// Creates a function-level key.
    fn function(function: &QualifiedFunctionName) -> Self {
        Self(function.to_string())
    }

    /// Creates a function-level key from its serialized name.
    pub fn function_name(function: &str) -> Self {
        Self(function.to_string())
    }

    /// Creates a case-level key from its stable expansion index.
    pub fn parameter_case(function: &QualifiedFunctionName, index: usize) -> Self {
        Self(format!("{function}[{index}]"))
    }

    /// Creates a case-level key from an already serialized function identity.
    pub fn parameter_case_name(function: &str, index: usize) -> Self {
        Self(format!("{function}[{index}]"))
    }

    /// Returns the qualified function portion without a case suffix.
    pub fn test_function_name(&self) -> &str {
        self.case_suffix()
            .map_or(self.0.as_str(), |(function, _)| function)
    }

    /// The statically scheduled case that owns this fixture-expanded result.
    pub fn fixture_case_group(&self) -> Option<usize> {
        let (_, case) = self.case_suffix()?;
        case.split_once(':')?.0.parse().ok()
    }

    fn case_suffix(&self) -> Option<(&str, &str)> {
        let (function, suffix) = self.0.rsplit_once('[')?;
        let case = suffix.strip_suffix(']')?;
        let is_index =
            |index: &str| !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit());
        let valid = case.split_once(':').map_or_else(
            || is_index(case),
            |(group, index)| is_index(group) && is_index(index),
        );
        valid.then_some((function, case))
    }

    /// Whether this key identifies one statically indexed parameter case.
    pub fn is_parameter_case(&self) -> bool {
        self.test_function_name().len() != self.0.len()
    }
}

impl std::borrow::Borrow<str> for TestCacheKey {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for TestCacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::fmt::Display for TestCacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for TestCacheKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

/// User-visible test identity, optionally specialized to one parameter variant.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QualifiedTestName {
    function_name: QualifiedFunctionName,
    parameters: Option<String>,
    case: Option<ParameterCase>,
}

impl QualifiedTestName {
    /// Creates an identity for an unparameterized test function.
    pub fn new(function_name: QualifiedFunctionName) -> Self {
        Self {
            function_name,
            parameters: None,
            case: None,
        }
    }

    /// Creates an identity with the rendered contents of its parameter list.
    pub fn with_parameters(function_name: QualifiedFunctionName, parameters: String) -> Self {
        Self {
            function_name,
            parameters: Some(parameters),
            case: None,
        }
    }

    /// Attaches rendered parameters while preserving the existing function and case identity.
    #[must_use]
    pub fn with_resolved_parameters(mut self, parameters: String) -> Self {
        self.parameters = Some(parameters);
        self
    }

    /// Attach a parametrize case index. Used for stable cache/duration keys
    /// that survive renaming of parameter values across runs.
    #[must_use]
    pub fn with_case_index(mut self, case_index: Option<usize>) -> Self {
        self.case = case_index.map(ParameterCase::Index);
        self
    }

    /// Return the underlying qualified function name.
    pub fn function_name(&self) -> &QualifiedFunctionName {
        &self.function_name
    }

    /// Returns the rendered contents of the parameter list, without parentheses.
    pub fn parameters(&self) -> Option<&str> {
        self.parameters.as_deref()
    }

    /// Attaches the full worker case identity.
    #[must_use]
    pub fn with_case(mut self, case: Option<ParameterCase>) -> Self {
        self.case = case;
        self
    }

    /// Returns the stable worker case identity, when known.
    pub fn case(&self) -> Option<ParameterCase> {
        self.case
    }

    /// Returns the worker's cache key independently of its rendered case name.
    pub fn cache_key(&self) -> TestCacheKey {
        self.case.map_or_else(
            || TestCacheKey::function(&self.function_name),
            |case| case.cache_key(&self.function_name),
        )
    }
}

impl std::fmt::Display for QualifiedTestName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.function_name)?;
        if let Some(parameters) = &self.parameters {
            write!(f, "({parameters})")?;
        }
        Ok(())
    }
}

/// Filesystem and import-system identities for the same Python module.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ModulePath {
    path: Utf8PathBuf,
    module_name: String,
}

impl ModulePath {
    /// Create a new module path by computing the dotted module name relative to `cwd`.
    pub fn new<P: Into<Utf8PathBuf>>(path: P, cwd: &Utf8Path) -> Option<Self> {
        let path = path.into();
        let module_name = module_name(cwd, path.as_ref())?;
        Some(Self { path, module_name })
    }

    /// Create a new module path with an explicit dotted module name.
    ///
    /// Use this when the module name cannot be computed from the file path
    /// (e.g. framework modules installed into a venv).
    pub fn new_with_name<P: Into<Utf8PathBuf>>(path: P, module_name: String) -> Self {
        Self {
            path: path.into(),
            module_name,
        }
    }

    /// Return the dotted module name (e.g., `"tests.test_add"`).
    pub fn module_name(&self) -> &str {
        self.module_name.as_str()
    }

    /// Return the filesystem path of this module.
    pub fn path(&self) -> &Utf8PathBuf {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function_name() -> QualifiedFunctionName {
        QualifiedFunctionName::new(
            "test_example".to_string(),
            ModulePath::new_with_name("test.py", "tests.test".to_string()),
        )
    }

    #[test]
    fn unparameterized_test_name_uses_function_identity() {
        let name = QualifiedTestName::new(function_name());

        assert_eq!(name.to_string(), "tests.test::test_example");
        assert_eq!(name.parameters(), None);
    }

    #[test]
    fn parameterized_test_name_appends_rendered_parameters() {
        let name = QualifiedTestName::with_parameters(function_name(), "value=1".to_string());

        assert_eq!(name.to_string(), "tests.test::test_example(value=1)");
        assert_eq!(name.parameters(), Some("value=1"));
    }

    #[test]
    fn parameterized_test_cache_key_uses_stable_case_index() {
        let name = QualifiedTestName::with_parameters(function_name(), "value=1".to_string())
            .with_case_index(Some(2));

        assert_eq!(name.cache_key().to_string(), "tests.test::test_example[2]");
        assert_eq!(
            name.cache_key().test_function_name(),
            "tests.test::test_example"
        );
    }

    #[test]
    fn cache_key_only_treats_terminal_numeric_brackets_as_case_index() {
        let path_with_brackets =
            TestCacheKey::function_name("tests/[generated]/test.py::test_case");
        let named_brackets = TestCacheKey::function_name("tests::test_case[name]");
        let indexed = TestCacheKey::function_name("tests/[generated]/test.py::test_case[12]");

        assert_eq!(
            path_with_brackets.test_function_name(),
            "tests/[generated]/test.py::test_case"
        );
        assert!(!path_with_brackets.is_parameter_case());
        assert_eq!(
            named_brackets.test_function_name(),
            "tests::test_case[name]"
        );
        assert!(!named_brackets.is_parameter_case());
        assert_eq!(
            indexed.test_function_name(),
            "tests/[generated]/test.py::test_case"
        );
        assert!(indexed.is_parameter_case());
    }
}
