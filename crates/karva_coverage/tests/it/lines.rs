use karva_coverage::CoverageSession;
use pyo3::Python;
use rstest::rstest;

use crate::common::TestContext;

#[rstest]
fn annotations_and_docstrings(#[values(false, true)] branches: bool) {
    let context = TestContext::new(
        r#"
        """Module documentation."""
        from typing import TYPE_CHECKING

        module_value: int = 1
        module_declaration: int

        if TYPE_CHECKING:
            import unavailable

        class Example:
            """Class documentation."""
            value: int

            def choose(self, flag):
                """Function documentation."""
                local: int
                if flag:
                    positive: int
                    return 1
                negative: int
                return 0
    "#,
    );
    let mut config = context.config();
    config.branches = branches;
    insta::with_settings!({snapshot_suffix => format!("{branches}-{}", TestContext::annotation_mode())}, {
        insta::assert_json_snapshot!(context.measure_with(r"
            Example().choose(True)
            Example().choose(False)
        ", &config));
    });
}

#[test]
fn unexecuted_source_is_included() {
    let context = TestContext::new(
        r"
        def covered():
            return 1
    ",
    );
    context.write_file(
        "unused.py",
        r"
        def uncovered(flag):
            if flag:
                return 1
            return 0
    ",
    );
    context.write_file(".venv/ignored.py", "ignored = 1");
    insta::assert_json_snapshot!(context.measure("covered()"));
}

#[rstest]
fn multiline_statements(#[values(false, true)] branches: bool) {
    let context = TestContext::new(
        r"
        def total():
            values = [
                1,
                2,
            ]
            return (
                sum(values)
            )
    ",
    );
    let mut config = context.config();
    config.branches = branches;
    insta::with_settings!({snapshot_suffix => branches.to_string()}, {
        insta::assert_json_snapshot!(context.measure_with("total()", &config));
    });
}

#[test]
fn excluded_clauses_and_partial_branches() {
    let context = TestContext::new(
        r"
        def excluded(flag):
            if flag:
                return 1
            else:  # pragma: no cover
                return 0

        def partial(flag):
            if flag:  # pragma: no branch
                return 1
            return 0

        def configured(flag):
            if flag:
                return 1
            else:  # deliberately omitted
                return 0
    ",
    );
    let mut config = context.config();
    config.exclude_lines = vec!["deliberately omitted".to_owned()];
    insta::assert_json_snapshot!(context.measure_with(
        r"
        excluded(True)
        partial(True)
        configured(True)
    ",
        &config
    ));
}

#[rstest]
fn loop_else_exclusion(
    #[values("for", "while")] loop_kind: &str,
    #[values("pragma", "pattern")] exclusion: &str,
) {
    let header = if loop_kind == "for" {
        "for value in values"
    } else {
        "while values"
    };
    let comment = if exclusion == "pragma" {
        "pragma: no cover"
    } else {
        "deliberately omitted"
    };
    let context = TestContext::new(&format!(
        r"
        def choose(values):
            {header}:
                break
            # An intervening comment must not hide the else header.
            else:  # {comment}
                return 0
            return 1
    "
    ));
    let mut config = context.config();
    if exclusion == "pattern" {
        config.exclude_lines = vec!["deliberately omitted".to_owned()];
    }
    insta::with_settings!({snapshot_suffix => format!("{loop_kind}-{exclusion}")}, {
        insta::assert_json_snapshot!(context.measure_with("choose([1])", &config));
    });
}

#[test]
fn configured_partial_branch() {
    let context = TestContext::new(
        r"
        def choose(flag):
            if flag:
                return 1
            return 0
    ",
    );
    let mut config = context.config();
    config.partial_branches = vec!["if flag:".to_owned()];
    insta::assert_json_snapshot!(context.measure_with("choose(True)", &config));
}

#[rstest]
fn invalid_measurement_patterns(#[values("exclude", "partial")] kind: &str) {
    let context = TestContext::new("value = 1");
    let mut config = context.config();
    if kind == "exclude" {
        config.exclude_lines = vec!["[".to_owned()];
    } else {
        config.partial_branches = vec!["[".to_owned()];
    }
    Python::initialize();
    let error = Python::attach(|py| {
        CoverageSession::start(py, context.root(), &config)
            .err()
            .expect("invalid measurement pattern")
    });
    insta::with_settings!({snapshot_suffix => kind}, {
        insta::assert_snapshot!(error.to_string());
    });
}
