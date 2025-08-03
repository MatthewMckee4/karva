#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct FunctionName {
    name: String,
    module: String,
}

impl FunctionName {
    pub(crate) const fn new(name: String, module: String) -> Self {
        Self { name, module }
    }

    pub(crate) fn function_name(&self) -> &str {
        &self.name
    }
}

impl std::fmt::Display for FunctionName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}::{}", self.module, self.name)
    }
}
