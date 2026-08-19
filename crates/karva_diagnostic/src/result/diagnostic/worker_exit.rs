use super::RenderedDiagnostic;
use crate::Severity;

/// Renders finalized runner-owned context as a transport-safe diagnostic.
pub(super) fn render(summary: &str, recovery: &str, stderr: &str) -> RenderedDiagnostic {
    let message = format!("{summary}. {recovery}");
    let mut rendered = format!("error[worker-crashed]: {summary}\n\n{recovery}\n");
    if !stderr.trim().is_empty() {
        rendered.push_str("\nWorker stderr:\n");
        rendered.push_str(stderr.trim_end());
        rendered.push('\n');
    }
    RenderedDiagnostic::new("worker-crashed", Severity::Error, &message, rendered)
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;

    use super::*;

    #[test]
    fn worker_exit_reports_startup_and_failure_limit_context() {
        let summary =
            "Worker 3 terminated with exit code 9 during startup before controller authentication";
        let recovery = "Karva preserved 0 completed test results from this assignment and did not retry 2 unstarted test selections because `--max-fail` was reached.";

        let diagnostic = render(summary, recovery, "startup log");

        assert_snapshot!(serde_json::to_string_pretty(&diagnostic).expect("serialize diagnostic"), @r#"
        {
          "code": "worker-crashed",
          "severity": "error",
          "message": "Worker 3 terminated with exit code 9 during startup before controller authentication. Karva preserved 0 completed test results from this assignment and did not retry 2 unstarted test selections because `--max-fail` was reached.",
          "rendered": "error[worker-crashed]: Worker 3 terminated with exit code 9 during startup before controller authentication\n\nKarva preserved 0 completed test results from this assignment and did not retry 2 unstarted test selections because `--max-fail` was reached.\n\nWorker stderr:\nstartup log\n"
        }
        "#);
    }

    #[test]
    fn worker_exit_reports_cleanup_after_completed_results() {
        let summary = "Worker 4 terminated with exit code 27 with no active test checkpoint";
        let recovery = "Karva preserved 1 completed test result from this assignment; no unstarted test selection remained. The worker exited after test execution, during cleanup or shutdown.";

        let diagnostic = render(summary, recovery, "");

        assert_snapshot!(serde_json::to_string_pretty(&diagnostic).expect("serialize diagnostic"), @r#"
        {
          "code": "worker-crashed",
          "severity": "error",
          "message": "Worker 4 terminated with exit code 27 with no active test checkpoint. Karva preserved 1 completed test result from this assignment; no unstarted test selection remained. The worker exited after test execution, during cleanup or shutdown.",
          "rendered": "error[worker-crashed]: Worker 4 terminated with exit code 27 with no active test checkpoint\n\nKarva preserved 1 completed test result from this assignment; no unstarted test selection remained. The worker exited after test execution, during cleanup or shutdown.\n"
        }
        "#);
    }
}
