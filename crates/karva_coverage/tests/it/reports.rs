use karva_coverage::{
    CoverageAnalysis, CoverageFilters, CoverageReportFormat, CoverageReportOptions,
};
use rstest::rstest;

use crate::common::{TestContext, report_snapshot};

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
    let actual = String::from_utf8(output).expect("UTF-8 report");
    match format {
        "text" => insta::assert_snapshot!(actual, @"

        Name        Stmts   Miss   Branch   BrPart   Cover   Missing
        ------------------------------------------------------------
        sample.py       4      1        2        1   66.67%   4, 2->4
        ------------------------------------------------------------
        TOTAL           4      1        2        1   66.67%
        "),
        "markdown" => insta::assert_snapshot!(actual, @"
        | Name | Stmts | Miss | Branch | BrPart | Cover | Missing |
        | --- | ---: | ---: | ---: | ---: | ---: | --- |
        | sample.py | 4 | 1 | 2 | 1 | 66.67% | 4, 2->4 |
        | **TOTAL** | 4 | 1 | 2 | 1 | **66.67%** |  |
        "),
        _ => insta::assert_snapshot!(actual, @"66.67"),
    }
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
    insta::assert_snapshot!(report_snapshot(&context.report(&[artifact], &filters)), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | []
    executed_lines    | [1, 2]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 0
    covered_lines            | 2
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 0
    num_partial_branches     | 0
    num_statements           | 2
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 0
    covered_lines            | 2
    missing_branches         | 0
    num_branches             | 0
    num_partial_branches     | 0
    num_statements           | 2
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
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
    let actual = context.normalize(&report);
    match format {
        "lcov" => insta::assert_snapshot!(actual, @"
        SF:sample.py
        DA:1,1
        DA:2,1
        DA:3,1
        DA:4,0
        LF:4
        LH:3
        BRDA:2,0,0,1
        BRDA:2,0,1,0
        BRF:2
        BRH:1
        end_of_record
        "),
        "xml" => insta::assert_snapshot!(actual, @r#"
        <?xml version="1.0" ?>
        <coverage version="1.0" timestamp="[TIME]" lines-valid="4" lines-covered="3" line-rate="0.7500" branches-covered="1" branches-valid="2" branch-rate="0.5000" complexity="0.0">
          <sources>
            <source>.</source>
          </sources>
          <packages>
            <package name="." line-rate="0.7500" branch-rate="0.5000" complexity="0.0">
              <classes>
                <class name="sample.py" filename="sample.py" line-rate="0.7500" branch-rate="0.5000" complexity="0.0">
                  <methods/>
                  <lines>
                    <line number="1" hits="1" branch="false"/>
                    <line number="2" hits="1" branch="true" condition-coverage="50% (1/2)"/>
                    <line number="3" hits="1" branch="false"/>
                    <line number="4" hits="0" branch="false"/>
                  </lines>
                </class>
              </classes>
            </package>
          </packages>
        </coverage>
        "#),
        _ => insta::assert_snapshot!(actual, @r##"
        <!DOCTYPE html>
        <html lang="en">
        <head>
          <meta charset="utf-8">
          <meta name="viewport" content="width=device-width, initial-scale=1">
          <title>sample.py — Coverage report</title>
          <style>body{font-family:system-ui,sans-serif;margin:2rem;color:#202124}table{border-collapse:collapse;width:100%}th,td{padding:.5rem;border-bottom:1px solid #ddd;text-align:left}td.num{text-align:right;font-variant-numeric:tabular-nums}code,.source{font-family:ui-monospace,SFMono-Regular,monospace}thead{background:#f5f5f5}h1{margin-top:0}a{color:#0969da}.source{display:block;overflow:auto;background:#f6f8fa;padding:1rem}.line{display:block;min-height:1.35em}.number{display:inline-block;width:3rem;text-align:right;color:#6e7781;text-decoration:none}.executed{background:#dafbe1}.missing{background:#ffebe9}.excluded{background:#f0f0f0;color:#6e7781}.partial{background:#fff8c5}.branches{color:#9a6700}.contexts{float:right;color:#57606a;font-size:.85em}</style>
        </head>
        <body>
          <p><a href="index.html">← Index</a></p>
          <h1><code>sample.py</code></h1>
          <p>Coverage: <strong>67%</strong></p>
          <pre class="source">
        <span class="line executed"><a id="L1" href="#L1" class="number">    1</a> <code>def choose(flag):</code></span>
        <span class="line partial"><a id="L2" href="#L2" class="number">    2</a> <code>    if flag:</code> <span class="branches">missing → 4</span></span>
        <span class="line executed"><a id="L3" href="#L3" class="number">    3</a> <code>        return &#39;&lt;covered&gt;&#39;</code></span>
        <span class="line missing"><a id="L4" href="#L4" class="number">    4</a> <code>    return &#39;missing&#39;</code></span>
          </pre>
        </body>
        </html>
        "##),
    }
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
    insta::assert_snapshot!(context.normalize(&error.to_string()), @"native coverage artifact `[PROJECT]/native.json` expected unchanged source `[PROJECT]/sample.py` with fingerprint 09da0217f9bb0259f7f19640805dbdc3");
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
    let actual = error.to_string();
    match kind {
        "include" => {
            insta::assert_snapshot!(actual, @"invalid coverage include glob `[`: error parsing glob '[': unclosed character class; missing ']'");
        }
        "omit" => {
            insta::assert_snapshot!(actual, @"invalid coverage omit glob `[`: error parsing glob '[': unclosed character class; missing ']'");
        }
        "context" => insta::assert_snapshot!(actual, @"
        invalid coverage context pattern: regex parse error:
            [
            ^
        error: unclosed character class
        "),
        _ => {
            insta::assert_snapshot!(actual, @"invalid coverage path alias `missing-target`: expected `FROM=TO`");
        }
    }
}
