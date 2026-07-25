use std::collections::BTreeMap;
use std::fmt::Write as _;

use anyhow::{Context as _, Result};
use camino::Utf8Path;
use karva_cache::AggregatedResults;
use karva_diagnostic::{RenderedDiagnostic, TestCaseAttempt, TestCaseOutcome, TestCaseResult};
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
    let suites = test_cases_by_module(&results.test_cases);
    let run_errors = results
        .run_diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .count();
    let total_time = results
        .test_cases
        .iter()
        .map(TestCaseResult::duration)
        .sum::<std::time::Duration>()
        .as_secs_f64();

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    writeln!(
        xml,
        "<testsuites name=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" errors=\"{}\" time=\"{total_time:.6}\">",
        escape_xml(&settings.report_name),
        results.stats.total() + run_errors,
        results.stats.failed(),
        results.stats.skipped(),
        results.stats.errors() + run_errors,
    )?;

    for (module_name, cases) in suites {
        write_suite(&mut xml, settings, module_name, cases)?;
    }
    write_run_diagnostics_suite(&mut xml, settings, &results.run_diagnostics)?;

    xml.push_str("</testsuites>\n");
    Ok(xml)
}

fn write_suite(
    xml: &mut String,
    settings: &JunitSettings,
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
    let errors = cases
        .iter()
        .filter(|case| case.outcome().is_error())
        .count();
    let time = cases
        .iter()
        .map(|case| case.duration())
        .sum::<std::time::Duration>()
        .as_secs_f64();

    writeln!(
        xml,
        "  <testsuite name=\"{}\" tests=\"{tests}\" failures=\"{failures}\" skipped=\"{skipped}\" errors=\"{errors}\" time=\"{time:.6}\">",
        escape_xml(module_name),
    )?;

    for case in cases {
        write_case(xml, settings, case)?;
    }

    xml.push_str("  </testsuite>\n");
    Ok(())
}

fn write_case(xml: &mut String, settings: &JunitSettings, case: &TestCaseResult) -> Result<()> {
    let time = junit_case_duration(case).as_secs_f64();
    let output = case.captured_output();
    let include_output = match case.outcome() {
        TestCaseOutcome::Passed => settings.store_success_output,
        TestCaseOutcome::Failed { .. } | TestCaseOutcome::Error { .. } => {
            settings.store_failure_output
        }
        TestCaseOutcome::Skipped { .. } => false,
    };
    let has_output = include_output
        && output.is_some_and(|output| !output.stdout().is_empty() || !output.stderr().is_empty());
    let has_reruns = case.attempts().len() > 1;
    let is_self_closing =
        matches!(case.outcome(), TestCaseOutcome::Passed) && !has_output && !has_reruns;

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
        TestCaseOutcome::Failed {
            diagnostic,
            related,
        } => {
            if case.attempts().len() > 1
                && let Some(first_attempt) = case.attempts().first()
                && let Some(first_diagnostic) = first_attempt.outcome().diagnostic()
            {
                write_diagnostic_element(
                    xml,
                    "failure",
                    first_diagnostic,
                    first_attempt
                        .outcome()
                        .related_diagnostics()
                        .iter()
                        .chain(related),
                    None,
                )?;
            } else {
                write_diagnostic_element(xml, "failure", diagnostic, related, None)?;
            }
        }
        TestCaseOutcome::Error {
            diagnostic,
            related,
        } => write_diagnostic_element(xml, "error", diagnostic, related, None)?,
        TestCaseOutcome::Skipped { reason } => {
            if let Some(reason) = reason {
                writeln!(xml, "      <skipped message=\"{}\"/>", escape_xml(reason))?;
            } else {
                xml.push_str("      <skipped/>\n");
            }
        }
    }
    write_attempts(xml, case)?;

    if has_output && let Some(output) = output {
        write_captured_stream(xml, "system-out", output.stdout())?;
        write_captured_stream(xml, "system-err", output.stderr())?;
    }

    xml.push_str("    </testcase>\n");
    Ok(())
}

fn write_attempts(xml: &mut String, case: &TestCaseResult) -> Result<()> {
    let attempts = case.attempts();
    if attempts.len() <= 1 {
        return Ok(());
    }
    match case.outcome() {
        TestCaseOutcome::Passed => {
            write_attempt_diagnostics(xml, "flakyFailure", &attempts[..attempts.len() - 1])
        }
        TestCaseOutcome::Failed { .. } => {
            write_attempt_diagnostics(xml, "rerunFailure", &attempts[1..])
        }
        TestCaseOutcome::Error { .. } | TestCaseOutcome::Skipped { .. } => {
            write_attempt_diagnostics(xml, "rerunFailure", &attempts[..attempts.len() - 1])
        }
    }
}

fn write_attempt_diagnostics(
    xml: &mut String,
    element: &str,
    attempts: &[TestCaseAttempt],
) -> Result<()> {
    for attempt in attempts {
        if let Some(diagnostic) = attempt.outcome().diagnostic() {
            write_diagnostic_element(
                xml,
                element,
                diagnostic,
                attempt.outcome().related_diagnostics(),
                Some(attempt.duration()),
            )?;
        }
    }
    Ok(())
}

fn write_diagnostic_element<'a>(
    xml: &mut String,
    element: &str,
    diagnostic: &RenderedDiagnostic,
    related: impl IntoIterator<Item = &'a RenderedDiagnostic>,
    duration: Option<std::time::Duration>,
) -> Result<()> {
    write!(
        xml,
        "      <{element} message=\"{}\" type=\"{}\"",
        escape_xml(diagnostic.message()),
        escape_xml(diagnostic.code()),
    )?;
    if let Some(duration) = duration {
        write!(xml, " time=\"{:.6}\"", duration.as_secs_f64())?;
    }
    write!(xml, ">{}", escape_xml(diagnostic.rendered()))?;
    for diagnostic in related {
        write!(xml, "{}", escape_xml(diagnostic.rendered()))?;
    }
    writeln!(xml, "</{element}>")?;
    Ok(())
}

fn write_run_diagnostics_suite(
    xml: &mut String,
    settings: &JunitSettings,
    diagnostics: &[karva_diagnostic::RenderedDiagnostic],
) -> Result<()> {
    let diagnostics = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .collect::<Vec<_>>();
    if diagnostics.is_empty() {
        return Ok(());
    }

    writeln!(
        xml,
        "  <testsuite name=\"{}::run\" tests=\"{}\" failures=\"0\" skipped=\"0\" errors=\"{}\" time=\"0.000000\">",
        escape_xml(&settings.report_name),
        diagnostics.len(),
        diagnostics.len(),
    )?;
    for (index, diagnostic) in diagnostics.iter().enumerate() {
        writeln!(
            xml,
            "    <testcase classname=\"karva.run\" name=\"{}-{}\" time=\"0.000000\">",
            escape_xml(diagnostic.code()),
            index + 1,
        )?;
        write_diagnostic_element(xml, "error", diagnostic, &[], None)?;
        xml.push_str("    </testcase>\n");
    }
    xml.push_str("  </testsuite>\n");
    Ok(())
}

fn junit_case_duration(case: &TestCaseResult) -> std::time::Duration {
    if case.attempts().len() > 1 {
        match case.outcome() {
            TestCaseOutcome::Passed => {
                return case
                    .attempts()
                    .last()
                    .map_or_else(|| case.duration(), TestCaseAttempt::duration);
            }
            TestCaseOutcome::Failed { .. } => {
                return case
                    .attempts()
                    .first()
                    .map_or_else(|| case.duration(), TestCaseAttempt::duration);
            }
            TestCaseOutcome::Error { .. } | TestCaseOutcome::Skipped { .. } => {}
        }
    }
    case.duration()
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
