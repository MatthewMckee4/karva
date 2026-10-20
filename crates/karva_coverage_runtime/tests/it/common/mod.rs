//! A temporary Python project measured through the public worker and report APIs.
//! Fixtures use Python's standard-library dedent and start on line one. Sessions
//! are serialized for cargo test; nextest isolates each test in its own process.

use std::ffi::CString;
use std::fmt::Write;
use std::sync::Mutex;

use camino::{Utf8Path, Utf8PathBuf};
use karva_coverage::native::{CoverageMode, NativeCoverage};
use karva_coverage_report::{CoverageAnalysis, CoverageFilters};
use karva_coverage_runtime::{CoverageConfig, CoveragePhase, CoverageSession};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;
use tempfile::TempDir;

// Tracing is interpreter-global; cargo test must serialize sessions just as nextest isolates them.
static PYTHON_SESSION: Mutex<()> = Mutex::new(());

pub struct TestContext {
    directory: TempDir,
}

impl TestContext {
    pub fn new(source: &str) -> Self {
        let context = Self {
            directory: tempfile::tempdir().expect("coverage project"),
        };
        context.write_file("sample.py", source);
        context
    }

    pub fn root(&self) -> &Utf8Path {
        Utf8Path::from_path(self.directory.path()).expect("UTF-8 project path")
    }

    pub fn write_file(&self, path: &str, source: &str) {
        let path = self.root().join(path);
        fs_err::create_dir_all(path.parent().expect("source parent")).expect("source directory");
        fs_err::write(path, source_text(source)).expect("write source");
    }

    pub fn annotation_mode() -> &'static str {
        Python::initialize();
        let minor = Python::attach(|py| {
            py.import("sys")?
                .getattr("version_info")?
                .get_item(1)?
                .extract::<u8>()
        })
        .expect("Python minor version");
        if minor >= 14 { "deferred" } else { "eager" }
    }

    pub fn normalize(&self, text: &str) -> String {
        text.replace('\\', "/")
            .replace(&self.root().as_str().replace('\\', "/"), "[PROJECT]")
    }

    pub fn config(&self) -> CoverageConfig {
        CoverageConfig {
            sources: vec![String::new()],
            data_file: self.root().join("worker.json"),
            contexts: false,
            static_context: None,
            branches: true,
            exclude_lines: Vec::new(),
            partial_branches: Vec::new(),
        }
    }

    pub fn measure(&self, calls: &str) -> Value {
        self.measure_with(calls, &self.config())
    }

    pub fn measure_with(&self, calls: &str, config: &CoverageConfig) -> Value {
        let artifact = self.collect(&[("test_sample", calls)], config);
        self.report(&[artifact], &CoverageFilters::default())
    }

    /// Executes source once, then named test bodies in one session, preserving phase attribution.
    pub fn collect(&self, tests: &[(&str, &str)], config: &CoverageConfig) -> NativeCoverage {
        self.collect_with(config, |py, session, globals| {
            for (name, calls) in tests {
                session.set_test_context(py, name, CoveragePhase::Run);
                execute(py, globals, calls)?;
            }
            Ok(())
        })
    }

    pub fn collect_with(
        &self,
        config: &CoverageConfig,
        run: impl FnOnce(Python<'_>, &CoverageSession, &Bound<'_, PyDict>) -> PyResult<()>,
    ) -> NativeCoverage {
        let _guard = PYTHON_SESSION.lock().expect("Python coverage session lock");
        Python::initialize();
        Python::attach(|py| -> PyResult<()> {
            let globals = PyDict::new(py);
            globals.set_item("__name__", "sample")?;
            globals.set_item("__file__", self.root().join("sample.py").as_str())?;
            let source =
                fs_err::read_to_string(self.root().join("sample.py")).expect("read sample source");
            let builtins = py.import("builtins")?;
            let code = builtins.call_method1(
                "compile",
                (source, self.root().join("sample.py").as_str(), "exec"),
            )?;
            let session = CoverageSession::start(py, self.root(), config)?;
            let execution = (|| {
                builtins.call_method1("exec", (&code, &globals))?;
                run(py, &session, &globals)?;
                Ok(())
            })();
            session.stop_and_save(py)?;
            execution
        })
        .expect("execute measured source");
        NativeCoverage::from_worker_files(
            self.root(),
            if config.branches {
                CoverageMode::Branch
            } else {
                CoverageMode::Line
            },
            &[&config.data_file],
        )
        .expect("worker artifact")
    }

    pub fn analysis(
        &self,
        artifacts: &[NativeCoverage],
        filters: &CoverageFilters,
    ) -> CoverageAnalysis {
        let paths: Vec<Utf8PathBuf> = artifacts
            .iter()
            .enumerate()
            .map(|(index, artifact)| {
                let path = self.root().join(format!("native-{index}.json"));
                artifact.write(&path).expect("write native artifact");
                path
            })
            .collect();
        CoverageAnalysis::load_native(self.root(), &paths, filters)
            .expect("analyze native artifacts")
            .expect("measured source")
    }

    pub fn report(&self, artifacts: &[NativeCoverage], filters: &CoverageFilters) -> Value {
        let analysis = self.analysis(artifacts, filters);
        let output = self.root().join("coverage.json");
        analysis.write_json(&output).expect("write JSON report");
        serde_json::from_str(&fs_err::read_to_string(output).expect("read JSON report"))
            .expect("JSON report")
    }
}

pub fn execute(py: Python<'_>, globals: &Bound<'_, PyDict>, calls: &str) -> PyResult<()> {
    let calls = CString::new(source_text(calls)).expect("test body without NUL");
    py.run(&calls, Some(globals), Some(globals))
}

/// Keeps every report field visible while rendering line lists and branch pairs on one row.
pub fn report_snapshot(report: &Value) -> String {
    fn inline(value: &Value) -> String {
        if let Value::Array(values) = value {
            let values = values.iter().map(inline).collect::<Vec<_>>().join(", ");
            format!("[{values}]")
        } else {
            value.to_string()
        }
    }

    fn table(value: &Value, heading: &str, output: &mut String) {
        let Value::Object(fields) = value else {
            output.push_str(&inline(value));
            return;
        };
        if fields.is_empty() {
            writeln!(output, "{heading}: {{}}\n").expect("snapshot table");
            return;
        }
        if fields.values().any(|value| !value.is_object()) {
            let width = fields
                .iter()
                .filter(|(_, value)| !value.is_object())
                .map(|(field, _)| field.len())
                .chain(std::iter::once("field".len()))
                .max()
                .expect("table header");
            writeln!(output, "{heading}\n{:width$} | value", "field").expect("snapshot header");
            for (field, value) in fields {
                if !value.is_object() {
                    writeln!(output, "{field:width$} | {}", inline(value)).expect("snapshot row");
                }
            }
            output.push('\n');
        }
        for (field, value) in fields {
            if value.is_object() {
                let heading = if heading.is_empty() {
                    field.clone()
                } else {
                    format!("{heading}.{field}")
                };
                table(value, &heading, output);
            }
        }
    }

    let mut output = String::new();
    table(report, "", &mut output);
    output
}

/// Removes the Rust fixture margin and its opening blank line; Python starts on line one.
fn source_text(source: &str) -> String {
    Python::initialize();
    let text = Python::attach(|py| {
        py.import("textwrap")?
            .call_method1("dedent", (source,))?
            .extract::<String>()
    })
    .expect("dedent Python fixture");
    text.strip_prefix('\n').unwrap_or(&text).to_owned()
}
