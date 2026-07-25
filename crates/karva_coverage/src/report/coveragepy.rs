use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;
use rusqlite::{Connection, Transaction, params};

use super::CoverageFilters;
use super::combined_rows;
use super::shared::{FileRow, total_percent};

const COVERAGE_SCHEMA_VERSION: u32 = 7;
const COVERAGE_SCHEMA: &str = "
    CREATE TABLE coverage_schema (
        version integer
    );
    CREATE TABLE meta (
        key text,
        value text,
        unique (key)
    );
    CREATE TABLE file (
        id integer primary key,
        path text,
        unique (path)
    );
    CREATE TABLE context (
        id integer primary key,
        context text,
        unique (context)
    );
    CREATE TABLE line_bits (
        file_id integer,
        context_id integer,
        numbits blob,
        foreign key (file_id) references file (id),
        foreign key (context_id) references context (id),
        unique (file_id, context_id)
    );
    CREATE TABLE arc (
        file_id integer,
        context_id integer,
        fromno integer,
        tono integer,
        foreign key (file_id) references file (id),
        foreign key (context_id) references context (id),
        unique (file_id, context_id, fromno, tono)
    );
    CREATE TABLE tracer (
        file_id integer primary key,
        tracer text,
        foreign key (file_id) references file (id)
    );
";

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
    let mut connection = Connection::open(output.as_std_path())
        .with_context(|| format!("failed to create coverage data file {output}"))?;
    let transaction = connection
        .transaction()
        .context("failed to start coverage data transaction")?;
    transaction
        .execute_batch(COVERAGE_SCHEMA)
        .context("failed to create coverage.py schema")?;
    transaction.execute(
        "INSERT INTO coverage_schema (version) VALUES (?)",
        [COVERAGE_SCHEMA_VERSION],
    )?;
    transaction.execute(
        "INSERT INTO meta (key, value) VALUES (?, ?)",
        ["version", "karva"],
    )?;
    transaction.execute(
        "INSERT INTO meta (key, value) VALUES (?, ?)",
        [
            "has_arcs",
            if rows.iter().any(|row| row.branches_enabled) {
                "1"
            } else {
                "0"
            },
        ],
    )?;

    let mut context_ids = BTreeMap::new();
    for row in rows {
        transaction.execute("INSERT INTO file (path) VALUES (?)", [&row.absolute_name])?;
        let file_id = transaction.last_insert_rowid();

        for (context, lines) in lines_by_context(row) {
            if lines.is_empty() {
                continue;
            }
            let context_id = context_id(&transaction, &mut context_ids, &context)?;
            transaction.execute(
                "INSERT INTO line_bits (file_id, context_id, numbits) VALUES (?, ?, ?)",
                params![file_id, context_id, nums_to_numbits(&lines)],
            )?;
        }

        for (context, arcs) in arcs_by_context(row) {
            if arcs.is_empty() {
                continue;
            }
            let context_id = context_id(&transaction, &mut context_ids, &context)?;
            for arc in arcs {
                transaction.execute(
                    "INSERT INTO arc (file_id, context_id, fromno, tono) VALUES (?, ?, ?, ?)",
                    params![file_id, context_id, arc.from, arc.to],
                )?;
            }
        }
    }

    transaction
        .commit()
        .context("failed to commit coverage data")
}

fn context_id(
    transaction: &Transaction<'_>,
    context_ids: &mut BTreeMap<String, i64>,
    context: &str,
) -> Result<i64> {
    if let Some(context_id) = context_ids.get(context) {
        return Ok(*context_id);
    }

    transaction.execute("INSERT INTO context (context) VALUES (?)", [context])?;
    let context_id = transaction.last_insert_rowid();
    context_ids.insert(context.to_string(), context_id);
    Ok(context_id)
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

fn arcs_by_context(row: &FileRow) -> BTreeMap<String, BTreeSet<crate::data::BranchArc>> {
    let mut arcs_by_context: BTreeMap<String, BTreeSet<crate::data::BranchArc>> = BTreeMap::new();

    for arc in &row.arcs {
        if let Some(contexts) = row.arc_contexts.get(arc)
            && !contexts.is_empty()
        {
            for context in contexts {
                arcs_by_context
                    .entry(context.clone())
                    .or_default()
                    .insert(*arc);
            }
        } else {
            arcs_by_context
                .entry(String::new())
                .or_default()
                .insert(*arc);
        }
    }

    arcs_by_context
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
    use crate::data::{BranchArc, BranchEntry, FileEntry, WorkerFile};

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
                        branches: None,
                    },
                ),
                (
                    unused.to_string(),
                    FileEntry {
                        executable: vec![1, 2],
                        executed: Vec::new(),
                        contexts: BTreeMap::new(),
                        branches: None,
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

    #[test]
    fn write_coveragepy_sqlite_writes_branch_arcs() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 temp dir");
        let worker_file = root.join("worker.json");
        let output = root.join(".coverage");
        let app = root.join("src/app.py");
        let worker = WorkerFile {
            files: BTreeMap::from([(
                app.to_string(),
                FileEntry {
                    executable: vec![1, 2, 3, 4],
                    executed: vec![1, 2, 3],
                    contexts: BTreeMap::new(),
                    branches: Some(BranchEntry {
                        possible: vec![BranchArc { from: 2, to: 3 }, BranchArc { from: 2, to: 4 }],
                        executed: vec![
                            BranchArc { from: -1, to: 1 },
                            BranchArc { from: 1, to: 2 },
                            BranchArc { from: 2, to: 3 },
                            BranchArc { from: 3, to: -1 },
                        ],
                        contexts: Vec::new(),
                    }),
                },
            )]),
        };
        fs::write(
            &worker_file,
            serde_json::to_vec(&worker).expect("serialize worker file"),
        )
        .expect("write worker file");

        write_coveragepy_sqlite(&root, &[worker_file], &output, &CoverageFilters::default())
            .expect("write coverage.py sqlite");

        let (has_arcs, arcs) = query_arc_data(&output, &app).expect("query arc data");

        assert_eq!(has_arcs, "1");
        assert_eq!(arcs, vec![(-1, 1), (1, 2), (2, 3), (3, -1)]);
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

    fn query_arc_data(output: &Utf8Path, app: &Utf8Path) -> PyResult<(String, Vec<(i32, i32)>)> {
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
        conn.execute("SELECT value FROM meta WHERE key = 'has_arcs'").fetchone()[0],
        list(conn.execute(
            """
                SELECT arc.fromno, arc.tono
                FROM arc
                JOIN file ON file.id = arc.file_id
                WHERE file.path = ?
                ORDER BY arc.fromno, arc.tono
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
