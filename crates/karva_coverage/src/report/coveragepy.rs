use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, bail};
use camino::Utf8Path;
use fs_err as fs;
use pyo3::types::PyAnyMethods;
use pyo3::{PyResult, Python};
use serde::Serialize;

use super::CoverageFilters;
use super::combined_rows;
use super::shared::{FileRow, total_percent};

const COVERAGE_SCHEMA_VERSION: u32 = 7;
const SQLITE_MODULE: &str = "karva._coverage_sqlite";

#[cfg(debug_assertions)]
const SQLITE_WRITER: &str = include_str!("../../../../python/karva/_coverage_sqlite.py");

#[derive(Serialize)]
struct SqlitePayload {
    files: Vec<SqliteFileRow>,
}

#[derive(Serialize)]
struct SqliteFileRow {
    path: String,
    contexts: Vec<SqliteContextRow>,
}

#[derive(Serialize)]
struct SqliteContextRow {
    context: String,
    numbits: Vec<u8>,
}

pub fn write_coveragepy_sqlite(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    output: &Utf8Path,
    filters: &CoverageFilters,
) -> Result<Option<f64>> {
    let Some((_, rows)) = combined_rows(cwd, files, false, filters)? else {
        return Ok(None);
    };
    let total_pct = total_percent(&rows);

    if let Some(parent) = output.parent()
        && !parent.as_str().is_empty()
    {
        fs::create_dir_all(parent.as_std_path())
            .with_context(|| format!("failed to create coverage data directory {parent}"))?;
    }
    if output.exists() {
        fs::remove_file(output.as_std_path())
            .with_context(|| format!("failed to replace existing coverage data file {output}"))?;
    }

    write_sqlite_file(output, &rows)?;

    Ok(Some(total_pct))
}

fn write_sqlite_file(output: &Utf8Path, rows: &[FileRow]) -> Result<()> {
    let payload_json = serde_json::to_string(&sqlite_payload(rows))
        .context("failed to serialize coverage.py data")?;
    let python = python_executable().context("failed to locate Python for coverage data file")?;

    let output_status =
        run_python_sqlite_writer(&python, &["-m", SQLITE_MODULE], output, &payload_json)?;
    if output_status.status.success() {
        return Ok(());
    }

    #[cfg(debug_assertions)]
    if module_not_found(&output_status.stderr) {
        let output_status =
            run_python_sqlite_writer(&python, &["-c", SQLITE_WRITER], output, &payload_json)?;
        if output_status.status.success() {
            return Ok(());
        }
        return python_writer_error(&output_status);
    }

    python_writer_error(&output_status)
}

fn run_python_sqlite_writer(
    python: &str,
    mode_args: &[&str],
    output: &Utf8Path,
    payload_json: &str,
) -> Result<Output> {
    let mut child = Command::new(python)
        .args(mode_args)
        .arg(output.as_str())
        .arg(COVERAGE_SCHEMA_VERSION.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start Python sqlite writer `{python}`"))?;

    let stdin = child
        .stdin
        .as_mut()
        .context("failed to open Python sqlite writer stdin")?;
    stdin
        .write_all(payload_json.as_bytes())
        .context("failed to send coverage.py data to Python sqlite writer")?;

    child
        .wait_with_output()
        .context("failed to wait for Python sqlite writer")
}

#[cfg(debug_assertions)]
fn module_not_found(stderr: &[u8]) -> bool {
    String::from_utf8_lossy(stderr).contains("No module named")
}

fn python_writer_error(output_status: &Output) -> Result<()> {
    let stderr = String::from_utf8_lossy(&output_status.stderr);
    bail!("Python sqlite writer failed: {}", stderr.trim());
}

fn sqlite_payload(rows: &[FileRow]) -> SqlitePayload {
    SqlitePayload {
        files: rows
            .iter()
            .map(|row| SqliteFileRow {
                path: row.absolute_name.clone(),
                contexts: lines_by_context(row)
                    .into_iter()
                    .filter(|(_, lines)| !lines.is_empty())
                    .map(|(context, lines)| SqliteContextRow {
                        context,
                        numbits: nums_to_numbits(&lines),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn python_executable() -> Result<String> {
    Python::initialize();
    let executable = Python::attach(|py| -> PyResult<String> {
        py.import("sys")?.getattr("executable")?.extract()
    })?;
    if is_python_executable(&executable) && python_has_sqlite(&executable) {
        return Ok(executable);
    }

    for candidate in python_candidates() {
        if python_has_sqlite(candidate) {
            return Ok((*candidate).to_string());
        }
    }

    Ok(executable)
}

fn is_python_executable(executable: &str) -> bool {
    Utf8Path::new(executable)
        .file_stem()
        .is_some_and(|stem| stem.starts_with("python"))
}

fn python_has_sqlite(executable: &str) -> bool {
    Command::new(executable)
        .arg("-c")
        .arg("import sqlite3")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(windows)]
fn python_candidates() -> &'static [&'static str] {
    &["python", "py"]
}

#[cfg(not(windows))]
fn python_candidates() -> &'static [&'static str] {
    &["python3", "python"]
}

fn lines_by_context(row: &FileRow) -> BTreeMap<String, BTreeSet<u32>> {
    let mut lines_by_context: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();

    for line in &row.executed {
        if let Some(contexts) = row.contexts.get(line)
            && !contexts.is_empty()
        {
            for context in contexts {
                lines_by_context
                    .entry(context.clone())
                    .or_default()
                    .insert(*line);
            }
        } else {
            lines_by_context
                .entry(String::new())
                .or_default()
                .insert(*line);
        }
    }

    lines_by_context
}

fn nums_to_numbits(lines: &BTreeSet<u32>) -> Vec<u8> {
    let Some(max_line) = lines.iter().next_back() else {
        return Vec::new();
    };
    let byte_len = usize::try_from((max_line / 8) + 1).expect("u32 line numbers fit into usize");
    let mut bits = vec![0_u8; byte_len];
    for line in lines {
        let byte_index = usize::try_from(line / 8).expect("u32 line numbers fit into usize");
        bits[byte_index] |= 1_u8 << (line % 8);
    }
    bits
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use pyo3::prelude::*;
    use pyo3::types::PyDict;

    use super::*;
    use crate::data::{FileEntry, WorkerFile};

    type CoverageData = (u32, String, u32, Vec<(String, String)>);

    #[test]
    fn nums_to_numbits_matches_coverage_py_format() {
        assert_eq!(nums_to_numbits(&BTreeSet::from([1, 2, 3, 7])), [0x8e]);
    }

    #[test]
    fn write_coveragepy_sqlite_writes_schema_and_contexts() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 temp dir");
        let worker_file = root.join("worker.json");
        let output = root.join(".coverage");
        let app = root.join("src/app.py");
        let unused = root.join("src/unused.py");
        let worker = WorkerFile {
            files: BTreeMap::from([
                (
                    app.to_string(),
                    FileEntry {
                        executable: vec![1, 2, 3, 6],
                        executed: vec![1, 2, 3, 6],
                        contexts: BTreeMap::from([
                            (
                                2,
                                BTreeSet::from([
                                    "test_mod::test_one".to_string(),
                                    "test_mod::test_two".to_string(),
                                ]),
                            ),
                            (
                                3,
                                BTreeSet::from([
                                    "test_mod::test_one".to_string(),
                                    "test_mod::test_two".to_string(),
                                ]),
                            ),
                            (6, BTreeSet::from(["test_mod::test_one".to_string()])),
                        ]),
                    },
                ),
                (
                    unused.to_string(),
                    FileEntry {
                        executable: vec![1, 2],
                        executed: Vec::new(),
                        contexts: BTreeMap::new(),
                    },
                ),
            ]),
        };
        fs::write(
            &worker_file,
            serde_json::to_vec(&worker).expect("serialize worker file"),
        )
        .expect("write worker file");

        write_coveragepy_sqlite(&root, &[worker_file], &output, &CoverageFilters::default())
            .expect("write coverage.py sqlite");

        let (schema_version, has_arcs, file_count, rows) =
            query_coverage_data(&output, &app).expect("query coverage data");

        assert_eq!(schema_version, COVERAGE_SCHEMA_VERSION);

        assert_eq!(has_arcs, "0");

        assert_eq!(file_count, 2);

        let rows: BTreeMap<_, _> = rows.into_iter().collect();

        assert_eq!(
            rows,
            BTreeMap::from([
                (String::new(), "02".to_string()),
                ("test_mod::test_one".to_string(), "4C".to_string()),
                ("test_mod::test_two".to_string(), "0C".to_string()),
            ])
        );
    }

    fn query_coverage_data(output: &Utf8Path, app: &Utf8Path) -> PyResult<CoverageData> {
        Python::initialize();
        Python::attach(|py| {
            let locals = PyDict::new(py);
            locals.set_item("output", output.as_str())?;
            locals.set_item("app", app.as_str())?;
            py.run(
                cr#"
import sqlite3

conn = sqlite3.connect(output)
try:
    result = (
        conn.execute("SELECT version FROM coverage_schema").fetchone()[0],
        conn.execute("SELECT value FROM meta WHERE key = 'has_arcs'").fetchone()[0],
        conn.execute("SELECT COUNT(*) FROM file").fetchone()[0],
        list(conn.execute(
            """
                SELECT context.context, hex(line_bits.numbits)
                FROM line_bits
                JOIN context ON context.id = line_bits.context_id
                JOIN file ON file.id = line_bits.file_id
                WHERE file.path = ?
                ORDER BY context.context
                """,
            (app,),
        )),
    )
finally:
    conn.close()
"#,
                None,
                Some(&locals),
            )?;
            locals
                .get_item("result")?
                .expect("result should be set")
                .extract()
        })
    }
}
