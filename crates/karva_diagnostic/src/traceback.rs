use std::collections::HashMap;

use camino::{Utf8Path, Utf8PathBuf};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyTraceback;
use ruff_source_file::{OneIndexed, SourceFile, SourceFileBuilder};
use ruff_text_size::{TextRange, TextSize};

/// Frames extracted directly from a Python exception traceback.
#[derive(Debug, Clone)]
pub struct Traceback {
    /// Frames ordered from the outermost call to the point where the exception was raised.
    pub frames: Vec<TracebackFrame>,
}

#[derive(Debug, Clone)]
/// One Python traceback frame with optional loaded source context.
pub struct TracebackFrame {
    /// Python callable name reported for the frame.
    pub function_name: String,

    /// Source file referenced by the frame.
    pub file_path: Utf8PathBuf,

    /// One-indexed source line reported by Python.
    pub line_number: OneIndexed,

    /// Loaded source and exact line range when the file remains readable.
    pub source: Option<TracebackFrameSource>,
}

#[derive(Debug, Clone)]
/// Source material used to render one traceback frame diagnostic.
pub struct TracebackFrameSource {
    /// Ruff source-file snapshot for diagnostic rendering.
    pub source_file: SourceFile,

    /// Byte range covering the reported source line.
    pub location: TextRange,
}

#[derive(Debug, PartialEq)]
struct TracebackLocation {
    file_path: Utf8PathBuf,
    line_number: OneIndexed,
    function_name: String,
}

impl Traceback {
    /// Extracts frames using the known source snapshot and loading each external path once.
    /// Source snapshots and failed reads are shared only within this traceback.
    pub fn from_error_with_source(
        py: Python,
        error: &PyErr,
        source_file: &SourceFile,
    ) -> Option<Self> {
        Self::from_error_with_fallback(py, error, Some(source_file))
    }

    fn from_error_with_fallback(
        py: Python,
        error: &PyErr,
        source_file: Option<&SourceFile>,
    ) -> Option<Self> {
        if let Some(traceback) = error.traceback(py) {
            let locations = match get_traceback_locations(&traceback) {
                Ok(locations) => locations,
                Err(err) => {
                    tracing::warn!("failed to extract Python traceback: {err}");
                    return None;
                }
            };
            let mut sources = HashMap::new();
            let frames = locations
                .into_iter()
                .map(|frame| {
                    let source = get_source_file_and_range(&frame, source_file, &mut sources).map(
                        |(source_file, location)| TracebackFrameSource {
                            source_file,
                            location,
                        },
                    );
                    TracebackFrame {
                        function_name: frame.function_name,
                        file_path: frame.file_path,
                        line_number: frame.line_number,
                        source,
                    }
                })
                .collect::<Vec<_>>();

            (!frames.is_empty()).then_some(Self { frames })
        } else {
            None
        }
    }
}

/// Prefers the known source, otherwise memoizes each path's source or failed read.
/// Each frame keeps its own line range even when it shares a source snapshot.
fn get_source_file_and_range(
    traceback_location: &TracebackLocation,
    fallback_source_file: Option<&SourceFile>,
    sources: &mut HashMap<Utf8PathBuf, Option<SourceFile>>,
) -> Option<(SourceFile, TextRange)> {
    if let Some(source_file) = fallback_source_file
        && source_file_matches_path(source_file, &traceback_location.file_path)
    {
        let text_range =
            calculate_line_range(source_file.source_text(), traceback_location.line_number)?;
        return Some((source_file.clone(), text_range));
    }

    let source_file = sources
        .entry(traceback_location.file_path.clone())
        .or_insert_with(|| {
            let source_text = std::fs::read_to_string(&traceback_location.file_path).ok()?;
            Some(
                SourceFileBuilder::new(traceback_location.file_path.as_str(), source_text).finish(),
            )
        })
        .as_ref()?;
    let text_range =
        calculate_line_range(source_file.source_text(), traceback_location.line_number)?;

    Some((source_file.clone(), text_range))
}

fn source_file_matches_path(source_file: &SourceFile, traceback_path: &Utf8Path) -> bool {
    let source_path = Utf8Path::new(source_file.name());
    source_path == traceback_path
        || source_path.ends_with(traceback_path)
        || traceback_path.ends_with(source_path)
}

/// Reads the traceback chain without losing filename characters or repeated frames.
fn get_traceback_locations(traceback: &Bound<'_, PyTraceback>) -> PyResult<Vec<TracebackLocation>> {
    let mut traceback = traceback.clone().into_any();
    let mut locations = Vec::new();
    while !traceback.is_none() {
        let code = traceback.getattr("tb_frame")?.getattr("f_code")?;
        let line_number = traceback.getattr("tb_lineno")?.extract()?;
        let line_number = OneIndexed::new(line_number)
            .ok_or_else(|| PyValueError::new_err("traceback line number must be positive"))?;
        locations.push(TracebackLocation {
            file_path: Utf8PathBuf::from(code.getattr("co_filename")?.extract::<String>()?),
            line_number,
            function_name: code.getattr("co_name")?.extract()?,
        });
        traceback = traceback.getattr("tb_next")?;
    }
    Ok(locations)
}

/// Calculate the `TextRange` for a specific line in the source text
#[expect(clippy::cast_possible_truncation)]
fn calculate_line_range(source_text: &str, line_number: OneIndexed) -> Option<TextRange> {
    let target_line = line_number.to_zero_indexed();
    let mut current_line = 0;
    let mut line_start = TextSize::default();

    for (idx, ch) in source_text.char_indices() {
        if current_line == target_line {
            // Find the end of this line
            let remaining = &source_text[idx..];
            let line_end = remaining.find('\n').map_or_else(
                || TextSize::new(source_text.len() as u32),
                |newline_pos| TextSize::new((idx + newline_pos) as u32),
            );

            // Trim the line to remove leading/trailing whitespace for the range
            let line_text = &source_text[line_start.to_usize()..line_end.to_usize()];
            let leading_whitespace = line_text.len() - line_text.trim_start().len();
            let content_length = line_text.trim().len();

            let range_start = line_start + TextSize::new(leading_whitespace as u32);
            let range_end = range_start + TextSize::new(content_length as u32);

            return Some(TextRange::new(range_start, range_end));
        }

        if ch == '\n' {
            current_line += 1;
            line_start = TextSize::new((idx + 1) as u32);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use std::ffi::CString;

    use rstest::rstest;
    use tempfile::tempdir;

    use super::*;

    #[rstest]
    fn extracts_frames_and_source(
        #[values("test.py", "test_\"quoted\".py", indoc! {r"
            test_
            newline.py"})]
        filename: &str,
    ) -> PyResult<()> {
        Python::initialize();
        Python::attach(|py| {
            let code = indoc! {cr"
                def outer():
                    inner()
                def inner():
                    raise ValueError('boom')
            "};
            let filename_cstr = CString::new(filename)?;
            let module = PyModule::from_code(py, code, &filename_cstr, c"test_traceback")?;
            let error = module
                .getattr("outer")?
                .call0()
                .expect_err("test must raise");
            let source_file =
                SourceFileBuilder::new(filename, code.to_str().expect("Python source is UTF-8"))
                    .finish();
            let traceback = Traceback::from_error_with_source(py, &error, &source_file)
                .expect("exception must retain its traceback");

            assert_eq!(traceback.frames.len(), 2);
            let outer = &traceback.frames[0];
            let inner = &traceback.frames[1];
            assert_eq!(outer.file_path.as_str(), filename);
            assert_eq!(outer.function_name, "outer");
            assert_eq!(outer.line_number.get(), 2);
            assert_eq!(inner.file_path.as_str(), filename);
            assert_eq!(inner.function_name, "inner");
            assert_eq!(inner.line_number.get(), 4);
            let source = inner
                .source
                .as_ref()
                .expect("source context must be retained");
            assert_eq!(source.source_file, source_file);
            assert_eq!(
                source.source_file.slice(source.location),
                "raise ValueError('boom')"
            );
            Ok(())
        })
    }

    #[test]
    fn retains_repeated_recursive_frames() -> PyResult<()> {
        Python::initialize();
        Python::attach(|py| {
            let code = indoc! {cr"
                def recurse(depth):
                    if depth:
                        recurse(depth - 1)
                    else:
                        raise ValueError('boom')
            "};
            let module = PyModule::from_code(py, code, c"recursive.py", c"test_traceback")?;
            let error = module
                .getattr("recurse")?
                .call1((5,))
                .expect_err("test must raise");
            let source_file = SourceFileBuilder::new(
                "recursive.py",
                code.to_str().expect("Python source is UTF-8"),
            )
            .finish();
            let traceback = Traceback::from_error_with_source(py, &error, &source_file)
                .expect("exception must retain its traceback");

            assert_eq!(traceback.frames.len(), 6);
            assert!(
                traceback
                    .frames
                    .iter()
                    .all(|frame| frame.function_name == "recurse")
            );
            assert_eq!(traceback.frames[0].line_number.get(), 3);
            assert_eq!(traceback.frames[5].line_number.get(), 5);
            Ok(())
        })
    }

    #[rstest]
    #[case(0, "outer", 2, "inner()")]
    #[case(1, "inner", 4, "raise ValueError('boom')")]
    fn external_source_keeps_each_frame(
        #[values(true, false)] readable: bool,
        #[case] index: usize,
        #[case] function: &str,
        #[case] line: usize,
        #[case] text: &str,
    ) -> PyResult<()> {
        let temp = tempdir().expect("temporary directory");
        let path = Utf8Path::from_path(temp.path())
            .expect("UTF-8 path")
            .join("helpers.py");
        let code = c"def outer():\n    inner()\ndef inner():\n    raise ValueError('boom')\n";
        if readable {
            std::fs::write(&path, code.to_bytes()).expect("write helper source");
        }
        let filename = CString::new(path.as_str())?;
        Python::initialize();
        Python::attach(|py| {
            let module = PyModule::from_code(py, code, &filename, c"test_external_traceback")?;
            let error = module
                .getattr("outer")?
                .call0()
                .expect_err("test must raise");
            let traceback = Traceback::from_error_with_fallback(py, &error, None)
                .expect("exception must retain its traceback");

            assert_eq!(traceback.frames.len(), 2);
            let frame = &traceback.frames[index];
            assert_eq!(frame.file_path, path);
            assert_eq!(frame.function_name, function);
            assert_eq!(frame.line_number.get(), line);
            assert_eq!(frame.source.is_some(), readable);
            if let Some(source) = &frame.source {
                assert_eq!(source.source_file.slice(source.location), text);
            }
            Ok(())
        })
    }

    #[test]
    fn exception_without_traceback_has_no_frames() {
        Python::initialize();
        Python::attach(|py| {
            let error = PyValueError::new_err("not raised");
            assert!(Traceback::from_error_with_fallback(py, &error, None).is_none());
        });
    }

    mod calculate_line_range_tests {
        use super::*;

        #[test]
        fn test_calculate_line_range_first_line() {
            let source = indoc! {r"
                line 1
                line 2
                line 3"};
            insta::assert_debug_snapshot!(calculate_line_range(source, OneIndexed::new(1).unwrap()), @"
            Some(
                0..6,
            )
            ");
        }

        #[test]
        fn test_calculate_line_range_middle_line() {
            let source = indoc! {r"
                line 1
                line 2
                line 3"};
            insta::assert_debug_snapshot!(calculate_line_range(source, OneIndexed::new(2).unwrap()), @"
            Some(
                7..13,
            )
            ");
        }

        #[test]
        fn test_calculate_line_range_last_line() {
            let source = indoc! {r"
                line 1
                line 2
                line 3"};
            insta::assert_debug_snapshot!(calculate_line_range(source, OneIndexed::new(3).unwrap()), @"
            Some(
                14..20,
            )
            ");
        }

        /// "indented line" is 13 characters, starting at position 11 (after 4 spaces),
        /// and leading/trailing whitespace should be trimmed from the range.
        #[test]
        fn test_calculate_line_range_with_whitespace() {
            let source = indoc! {r"
                line 1
                    indented line
                line 3"};
            insta::assert_debug_snapshot!(calculate_line_range(source, OneIndexed::new(2).unwrap()), @"
            Some(
                11..24,
            )
            ");
        }

        #[test]
        fn test_calculate_line_range_out_of_bounds() {
            let source = indoc! {r"
                line 1
                line 2"};
            insta::assert_debug_snapshot!(calculate_line_range(source, OneIndexed::new(10).unwrap()), @"None");
        }

        #[test]
        fn test_calculate_line_range_single_line() {
            insta::assert_debug_snapshot!(calculate_line_range("single line", OneIndexed::new(1).unwrap()), @"
            Some(
                0..11,
            )
            ");
        }
    }

    mod source_file_range_tests {
        use super::*;

        #[test]
        fn uses_matching_fallback_source_file() {
            let location = TracebackLocation {
                file_path: Utf8PathBuf::from("/tmp/project/tests/test_example.py"),
                line_number: OneIndexed::from_zero_indexed(1),
                function_name: "test_example".to_string(),
            };
            let source_file = SourceFileBuilder::new(
                "tests/test_example.py",
                indoc! {r"
                    def test_example():
                        assert False
                "},
            )
            .finish();

            let resolved =
                get_source_file_and_range(&location, Some(&source_file), &mut HashMap::new());
            assert!(resolved.is_some());
            let Some((resolved_source_file, range)) = resolved else {
                return;
            };

            assert_eq!(resolved_source_file, source_file);
            assert_eq!(resolved_source_file.slice(range), "assert False");
        }

        #[test]
        fn ignores_fallback_source_file_for_different_path() {
            let location = TracebackLocation {
                file_path: Utf8PathBuf::from("/tmp/project/tests/test_example.py"),
                line_number: OneIndexed::from_zero_indexed(1),
                function_name: "test_example".to_string(),
            };
            let source_file = SourceFileBuilder::new(
                "tests/test_other.py",
                indoc! {r"
                    def test_other():
                        assert False
                "},
            )
            .finish();

            assert!(
                get_source_file_and_range(&location, Some(&source_file), &mut HashMap::new())
                    .is_none()
            );
        }

        #[test]
        fn shares_source_snapshot_between_frames_but_not_tracebacks() {
            let temp = tempdir().expect("temporary directory");
            let path = Utf8Path::from_path(temp.path())
                .expect("UTF-8 path")
                .join("helpers.py");
            std::fs::write(
                &path,
                indoc! {r"
                first line
                    second line
            "},
            )
            .expect("write original source");
            let mut location = TracebackLocation {
                file_path: path.clone(),
                line_number: OneIndexed::from_zero_indexed(0),
                function_name: "helper".to_string(),
            };
            let mut sources = HashMap::new();
            let (first, first_range) = get_source_file_and_range(&location, None, &mut sources)
                .expect("first frame source");
            assert_eq!(first.slice(first_range), "first line");

            std::fs::write(
                &path,
                indoc! {r"
                updated first
                    updated second
            "},
            )
            .expect("replace source on disk");
            location.line_number = OneIndexed::from_zero_indexed(1);
            let (second, second_range) = get_source_file_and_range(&location, None, &mut sources)
                .expect("second frame source");
            assert_eq!(second, first);
            assert_eq!(second.slice(second_range), "second line");

            let (fresh, fresh_range) =
                get_source_file_and_range(&location, None, &mut HashMap::new())
                    .expect("new traceback source");
            assert_eq!(fresh.slice(fresh_range), "updated second");
        }

        #[test]
        fn shares_failed_reads_between_frames_but_not_tracebacks() {
            let temp = tempdir().expect("temporary directory");
            let path = Utf8Path::from_path(temp.path())
                .expect("UTF-8 path")
                .join("missing.py");
            let location = TracebackLocation {
                file_path: path.clone(),
                line_number: OneIndexed::from_zero_indexed(0),
                function_name: "helper".to_string(),
            };
            let mut sources = HashMap::new();
            assert!(get_source_file_and_range(&location, None, &mut sources).is_none());
            std::fs::write(&path, "new source\n").expect("create source on disk");
            assert!(get_source_file_and_range(&location, None, &mut sources).is_none());
            assert!(get_source_file_and_range(&location, None, &mut HashMap::new()).is_some());
        }
    }
}
