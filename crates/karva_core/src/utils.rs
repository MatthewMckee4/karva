use pyo3::Python;
use ruff_python_ast::PythonVersion;
use ruff_source_file::{LineIndex, PositionEncoding};
use ruff_text_size::TextSize;

#[must_use]
pub fn current_python_version() -> PythonVersion {
    PythonVersion::from(Python::with_gil(|py| {
        let inferred_python_version = py.version_info();
        (inferred_python_version.major, inferred_python_version.minor)
    }))
}

#[must_use]
pub fn from_text_size(offset: TextSize, source: &str) -> (usize, usize) {
    let index = LineIndex::from_source_text(source);
    let location = index.source_location(offset, source, PositionEncoding::Utf8);
    (location.line.get(), location.character_offset.get())
}
