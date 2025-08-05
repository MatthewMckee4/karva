/// Represents a fully qualified function name including its module path.
///
/// This structure ensures unique identification of test functions across
/// the entire test suite by combining the function name with its module path.
/// This is essential for avoiding name conflicts and providing clear test
/// identification in reports and diagnostics.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct QualifiedFunctionName {
    /// The actual function name (e.g., `test_addition`)
    function_name: String,
    /// The module path (e.g., `tests.math.test_calculator`)
    module_path: String,
}

impl QualifiedFunctionName {
    /// Creates a new qualified function name.
    ///
    /// # Arguments
    /// * `function_name` - The actual function name
    /// * `module_path` - The fully qualified module path
    pub(crate) const fn new(function_name: String, module_path: String) -> Self {
        Self {
            function_name,
            module_path,
        }
    }

    /// Returns the function name portion (without module path).
    pub(crate) fn function_name(&self) -> &str {
        &self.function_name
    }
}

impl std::fmt::Display for QualifiedFunctionName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}::{}", self.module_path, self.function_name)
    }
}
