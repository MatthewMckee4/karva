use karva_coverage::{
    CoverageAnalysis, CoverageFilters, CoverageReportFormat, CoverageReportOptions,
};
use rstest::rstest;

use crate::common::TestContext;

#[rstest]
fn terminal_formats(#[values("text", "markdown", "total")] format: &str) {
    let context = TestContext::new(
        r"
        def choose(flag):
            if flag:
                return 1
            return 0
    ",
    );
    let artifact = context.collect(&[("test", "choose(True)")], &context.config());
    let analysis = context.analysis(&[artifact], &CoverageFilters::default());
    let mut output = Vec::new();
    analysis
        .write_report(
            &CoverageReportOptions {
                show_missing: true,
                precision: 2,
                format: match format {
                    "markdown" => CoverageReportFormat::Markdown,
                    "total" => CoverageReportFormat::Total,
                    _ => CoverageReportFormat::Text,
                },
                ..CoverageReportOptions::default()
            },
            &mut output,
        )
        .expect("render terminal report");
    insta::with_settings!({snapshot_suffix => format}, {
        insta::assert_snapshot!(String::from_utf8(output).expect("UTF-8 report"));
    });
}

#[test]
fn file_filters_apply_before_totals() {
    let context = TestContext::new(
        r"
        def covered():
            return 1
    ",
    );
    context.write_file("unused.py", "unused = 1");
    let artifact = context.collect(&[("test", "covered()")], &context.config());
    let filters = CoverageFilters::new(&[], &["unused.py".to_owned()]).expect("omit filter");
    insta::assert_json_snapshot!(context.report(&[artifact], &filters));
}

#[rstest]
fn exported_formats(#[values("lcov", "xml", "html")] format: &str) {
    let context = TestContext::new(
        r"
        def choose(flag):
            if flag:
                return '<covered>'
            return 'missing'
    ",
    );
    let artifact = context.collect(&[("test", "choose(True)")], &context.config());
    let analysis = context.analysis(&[artifact], &CoverageFilters::default());
    let output = context.root().join(format);
    let report = match format {
        "lcov" => {
            analysis.write_lcov(&output).expect("write LCOV");
            fs_err::read_to_string(output).expect("LCOV report")
        }
        "xml" => {
            analysis.write_cobertura_xml(&output).expect("write XML");
            let xml = fs_err::read_to_string(output).expect("XML report");
            regex::Regex::new(r#"timestamp="\d+""#)
                .expect("timestamp pattern")
                .replace_all(&xml, "timestamp=\"[TIME]\"")
                .into_owned()
        }
        _ => {
            analysis.write_html(&output).expect("write HTML");
            let source = fs_err::read_dir(output)
                .expect("HTML directory")
                .map(|entry| entry.expect("HTML entry").path())
                .find(|path| {
                    path.file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with("source-"))
                })
                .expect("HTML source page");
            fs_err::read_to_string(source).expect("HTML source report")
        }
    };
    insta::with_settings!({snapshot_suffix => format}, {
        insta::assert_snapshot!(context.normalize(&report));
    });
}

#[test]
fn changed_source_is_rejected() {
    let context = TestContext::new("value = 1");
    let artifact = context.collect(&[], &context.config());
    let path = context.root().join("native.json");
    artifact.write(&path).expect("write artifact");
    context.write_file("sample.py", "value = 2");
    let error = CoverageAnalysis::load_native(context.root(), &[path], &CoverageFilters::default())
        .expect_err("stale source must fail");
    insta::assert_snapshot!(context.normalize(&error.to_string()));
}

#[rstest]
fn invalid_filters(#[values("include", "omit", "context", "alias")] kind: &str) {
    let error = match kind {
        "include" => CoverageFilters::new(&["[".to_owned()], &[]).expect_err("invalid include"),
        "omit" => CoverageFilters::new(&[], &["[".to_owned()]).expect_err("invalid omit"),
        "context" => CoverageFilters::default()
            .with_contexts(&["[".to_owned()])
            .expect_err("invalid context"),
        _ => CoverageFilters::default()
            .with_path_aliases(&["missing-target".to_owned()])
            .expect_err("invalid alias"),
    };
    insta::with_settings!({snapshot_suffix => kind}, {
        insta::assert_snapshot!(error.to_string());
    });
}
