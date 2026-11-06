mod latest;
mod pool;

pub(super) use latest::LatestPool;
pub(super) use pool::Pool;

/// Receipt: analyzing and dropping a 2,048-term binary expression on macOS ARM64
/// debug builds overflowed 1, 2 and 4 MiB, and passed at 8 MiB. Reserve twice
/// that successful measurement; stack pages are committed only when touched.
pub(in crate::server) const SEMANTIC_STACK_BYTES: usize = 16 * 1024 * 1024;

#[cfg(test)]
pub(super) use pool::QUEUED_REQUEST_LIMIT;

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use karva_ide::{SourceAnalysisSettings, analyze_source};
    use ruff_python_ast::PythonVersion;

    #[test]
    fn deep_expression_stack_receipt() -> anyhow::Result<()> {
        std::thread::Builder::new()
            .stack_size(super::SEMANTIC_STACK_BYTES)
            .spawn(|| {
                let root = Utf8PathBuf::from("/project");
                let source = format!(
                    "def test_deep():\n    assert {}\n",
                    vec!["1"; 2048].join(" + ")
                );
                let analysis = analyze_source(
                    &root.join("test_deep.py"),
                    &root,
                    source,
                    &SourceAnalysisSettings {
                        python_version: PythonVersion::PY313,
                        test_function_prefix: "test".to_owned(),
                        try_import_fixtures: false,
                    },
                )
                .expect("deep source should parse and analyze");
                assert_eq!(analysis.diagnostics, []);
            })?
            .join()
            .map_err(|payload| crate::runtime::panic_error(payload.as_ref()))?;
        Ok(())
    }
}
