#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    Test,
    Fixture,
}

impl FunctionKind {
    pub(crate) const fn capitalised(self) -> &'static str {
        match self {
            Self::Test => "Test",
            Self::Fixture => "Fixture",
        }
    }
}

impl std::fmt::Display for FunctionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Test => write!(f, "test"),
            Self::Fixture => write!(f, "fixture"),
        }
    }
}
