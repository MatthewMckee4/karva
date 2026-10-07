//! Original document metadata attached to externally generated parameter cases.

use karva_diagnostic::TestCaseSource;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use ruff_source_file::{OneIndexed, SourceFile, SourceFileBuilder};
use ruff_text_size::{TextRange, TextSize};

/// Decoded document shared by every location derived from it.
#[derive(Debug, Clone)]
#[pyclass(from_py_object)]
pub struct SourceDocument {
    /// Immutable source text and line index, shared by reference-counted clones.
    source_file: SourceFile,
}

/// Validated original position of one externally generated test case.
#[derive(Debug, Clone)]
#[pyclass(from_py_object)]
pub struct SourceLocation {
    /// Document text retained without copying it per parameter case.
    pub(crate) source_file: SourceFile,

    /// UTF-8 byte range used by diagnostic rendering.
    pub(crate) range: TextRange,

    /// One-based character coordinates retained in transport-safe results.
    pub(crate) result_source: TestCaseSource,
}

#[pymethods]
impl SourceDocument {
    #[new]
    fn new(path: String, text: String) -> PyResult<Self> {
        if path.is_empty() {
            return Err(PyValueError::new_err(
                "Source document path cannot be empty",
            ));
        }
        Ok(Self {
            source_file: SourceFileBuilder::new(path, text).finish(),
        })
    }

    /// Validates a one-based line and Unicode character column.
    fn location(&self, line: usize, column: usize) -> PyResult<SourceLocation> {
        let source = self.source_file.to_source_code();
        let Some(row) = OneIndexed::new(line) else {
            return Err(PyValueError::new_err(
                "Source line must be at least 1; received 0",
            ));
        };
        if line > source.line_count() {
            return Err(PyValueError::new_err(format!(
                "Source line {line} exceeds document line count {}",
                source.line_count()
            )));
        }
        let range = source.line_range(row);
        let text = source.slice(range).trim_end_matches(['\r', '\n']);
        let character_count = text.chars().count();
        if column == 0 || column - 1 > character_count {
            return Err(PyValueError::new_err(format!(
                "Source column {column} must be in 1..={} on line {line}",
                character_count + 1
            )));
        }
        let (offset, width) = text
            .char_indices()
            .nth(column - 1)
            .map_or((text.len(), 0), |(offset, character)| {
                (offset, character.len_utf8())
            });
        let offset =
            TextSize::try_from(offset).map_err(|error| PyValueError::new_err(error.to_string()))?;
        let width =
            TextSize::try_from(width).map_err(|error| PyValueError::new_err(error.to_string()))?;
        Ok(SourceLocation {
            source_file: self.source_file.clone(),
            range: TextRange::at(range.start() + offset, width),
            result_source: TestCaseSource::new(self.source_file.name().to_string(), line, column),
        })
    }
}
