use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use anyhow::{Context as _, Result};
use camino::Utf8Path;
use karva_cache::AggregatedResults;
use karva_diagnostic::{
    CapturedTestOutput, TestCaseOutcome, TestCaseResult, captured_outputs_by_test,
};
use karva_metadata::JunitSettings;
use karva_project::path::absolute;

pub(super) fn write_junit_report(
    settings: &JunitSettings,
    results: &AggregatedResults,
    project_root: &Utf8Path,
) -> Result<()> {
    let Some(path) = settings.path.as_deref() else {
        return Ok(());
    };

    let output_path = absolute(path, project_root);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create JUnit report directory `{parent}`"))?;
    }

    let xml = build_junit_xml(settings, results)?;
    std::fs::write(&output_path, xml)
        .with_context(|| format!("failed to write JUnit report `{output_path}`"))?;

    Ok(())
}

fn build_junit_xml(settings: &JunitSettings, results: &AggregatedResults) -> Result<String> {
    let captured_outputs = captured_outputs_by_test(&results.captured_outputs);
    let suites = test_cases_by_module(&results.test_cases);
    let total_time = results
        .test_cases
        .iter()
        .map(|case| case.duration().as_secs_f64())
        .sum::<f64>();

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    writeln!(
        xml,
        "<testsuites name=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" errors=\"0\" time=\"{total_time:.6}\">",
        escape_xml(&settings.report_name),
        results.stats.total(),
        results.stats.failed(),
        results.stats.skipped(),
    )?;

    for (module_name, cases) in suites {
        write_suite(&mut xml, settings, &captured_outputs, module_name, cases)?;
    }

    xml.push_str("</testsuites>\n");
    Ok(xml)
}

fn write_suite(
    xml: &mut String,
    settings: &JunitSettings,
    captured_outputs: &HashMap<&str, &CapturedTestOutput>,
    module_name: &str,
    cases: Vec<&TestCaseResult>,
) -> Result<()> {
    let tests = cases.len();
    let failures = cases
        .iter()
        .filter(|case| case.outcome().is_failed())
        .count();
    let skipped = cases
        .iter()
        .filter(|case| case.outcome().is_skipped())
        .count();
    let time = cases
        .iter()
        .map(|case| case.duration().as_secs_f64())
        .sum::<f64>();

    writeln!(
        xml,
        "  <testsuite name=\"{}\" tests=\"{tests}\" failures=\"{failures}\" skipped=\"{skipped}\" errors=\"0\" time=\"{time:.6}\">",
        escape_xml(module_name),
    )?;

    for case in cases {
        write_case(xml, settings, captured_outputs, case)?;
    }

    xml.push_str("  </testsuite>\n");
    Ok(())
}

fn write_case(
    xml: &mut String,
    settings: &JunitSettings,
    captured_outputs: &HashMap<&str, &CapturedTestOutput>,
    case: &TestCaseResult,
) -> Result<()> {
    let time = case.duration().as_secs_f64();
    let output = captured_outputs.get(case.full_name()).copied();
    let include_output = match case.outcome() {
        TestCaseOutcome::Passed => settings.store_success_output,
        TestCaseOutcome::Failed => settings.store_failure_output,
        TestCaseOutcome::Skipped { .. } => false,
    };
    let has_output = include_output
        && output.is_some_and(|output| !output.stdout().is_empty() || !output.stderr().is_empty());
    let is_self_closing = matches!(case.outcome(), TestCaseOutcome::Passed) && !has_output;

    write!(
        xml,
        "    <testcase classname=\"{}\" name=\"{}\" time=\"{time:.6}\"",
        escape_xml(case.module_name()),
        escape_xml(case.name()),
    )?;

    if is_self_closing {
        xml.push_str("/>\n");
        return Ok(());
    }

    xml.push_str(">\n");
    match case.outcome() {
        TestCaseOutcome::Passed => {}
        TestCaseOutcome::Failed => {
            xml.push_str("      <failure message=\"test failed\"/>\n");
        }
        TestCaseOutcome::Skipped { reason } => {
            if let Some(reason) = reason {
                writeln!(xml, "      <skipped message=\"{}\"/>", escape_xml(reason))?;
            } else {
                xml.push_str("      <skipped/>\n");
            }
        }
    }

    if has_output && let Some(output) = output {
        write_captured_stream(xml, "system-out", output.stdout())?;
        write_captured_stream(xml, "system-err", output.stderr())?;
    }

    xml.push_str("    </testcase>\n");
    Ok(())
}

fn write_captured_stream(xml: &mut String, element: &str, content: &str) -> Result<()> {
    if content.is_empty() {
        return Ok(());
    }

    writeln!(xml, "      <{element}>{}</{element}>", escape_xml(content))?;
    Ok(())
}

fn test_cases_by_module(cases: &[TestCaseResult]) -> BTreeMap<&str, Vec<&TestCaseResult>> {
    let mut by_module = BTreeMap::new();
    for case in cases {
        by_module
            .entry(case.module_name())
            .or_insert_with(Vec::new)
            .push(case);
    }
    by_module
}

fn escape_xml(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            c if is_xml_char(c) => escaped.push(c),
            _ => escaped.push_str("&#xFFFD;"),
        }
    }
    escaped
}

fn is_xml_char(c: char) -> bool {
    matches!(
        u32::from(c),
        0x09 | 0x0A | 0x0D | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x1_0000..=0x10_FFFF
    )
}
