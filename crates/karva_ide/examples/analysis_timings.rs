//! Reproducible source-analysis timings: `cargo run -p karva_ide --release --example analysis_timings`.
use std::fmt::Write as _;
use std::io::{self, Write as _};
use std::sync::Arc;
use std::time::Instant;

use camino::Utf8Path;
use karva_collector::{CollectionSettings, collect_source};
use karva_ide::{
    SourceAnalysisSettings, WorkspaceSourceIndex, fixture_reference_hovers, fixture_references,
    fixture_target,
};
use ruff_python_ast::PythonVersion;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = io::stdout().lock();
    let root = Utf8Path::new("/project");
    let settings = SourceAnalysisSettings {
        python_version: PythonVersion::PY314,
        test_function_prefix: "test".into(),
        try_import_fixtures: true,
    };
    let collection = CollectionSettings {
        python_version: settings.python_version,
        test_function_prefix: "test",
        respect_ignore_files: true,
        collect_fixtures: true,
        collect_doctests: false,
    };
    let mut modules = Vec::new();
    for index in 0..100 {
        let path = root.join(format!("test_{index}.py"));
        let mut source = String::from(
            "from karva import fixture\n@fixture\ndef sample() -> int:\n    return len('hello')\n",
        );
        for test in 0..20 {
            write!(source, "def test_{test}(sample):\n    assert sample\n")?;
        }
        modules.push(Arc::new(
            collect_source(&path, root, source, &collection, &[]).ok_or("collection failed")?,
        ));
    }
    let cache = Arc::default();
    for phase in ["cold", "edit"] {
        let index = WorkspaceSourceIndex::from_shared_modules_with_cache(
            root.to_owned(),
            settings.clone(),
            modules.clone(),
            Arc::clone(&cache),
        );
        for pass in ["first", "cached"] {
            let start = Instant::now();
            let mut hints = 0;
            for module in &modules {
                let analysis = index.analyze(module.path.path()).ok_or("analysis failed")?;
                hints += fixture_reference_hovers(&analysis).len();
            }
            writeln!(
                output,
                "{phase}/{pass}: {:.3}ms, {hints} hints",
                start.elapsed().as_secs_f64() * 1000.0
            )?;
        }
        let analysis = index
            .analyze(modules[0].path.path())
            .ok_or("analysis failed")?;
        let target = fixture_target(
            &analysis,
            modules[0].fixture_function_defs[0].name.range.start(),
        )
        .ok_or("fixture target failed")?;
        let references = fixture_references(&index, &target, true);
        let start = Instant::now();
        for _ in 0..100 {
            assert_eq!(
                fixture_references(&index, &target, true).len(),
                references.len()
            );
        }
        writeln!(
            output,
            "{phase}/references: {:.3}ms for 100 requests, {} results",
            start.elapsed().as_secs_f64() * 1000.0,
            references.len()
        )?;
        let module = &modules[0];
        let source = module.source_text.replace("len('hello')", "len('changed')");
        modules[0] = Arc::new(
            collect_source(module.path.path(), root, source, &collection, &[])
                .ok_or("edit collection failed")?,
        );
    }
    Ok(())
}
