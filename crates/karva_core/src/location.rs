use camino::Utf8PathBuf;
use ruff_source_file::OneIndexed;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    path: Utf8PathBuf,
    line: OneIndexed,
}

impl Location {
    pub(crate) const fn new(file: Utf8PathBuf, line: OneIndexed) -> Self {
        Self { path: file, line }
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.path, self.line)
    }
}
