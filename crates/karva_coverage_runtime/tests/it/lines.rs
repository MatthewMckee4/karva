use karva_coverage_runtime::CoverageSession;
use pyo3::Python;
use rstest::rstest;

use crate::common::{TestContext, report_snapshot};

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
    let actual = report_snapshot(&context.measure_with(
        r"
            Example().choose(True)
            Example().choose(False)
        ",
        &config,
    ));
    match (branches, TestContext::annotation_mode()) {
        (false, "eager") => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field          | value
        excluded_lines | [7, 8]
        executed_lines | [2, 4, 5, 10, 12, 14, 17, 19, 21]
        missing_lines  | []

        files.sample.py.summary
        field           | value
        covered_lines   | 9
        excluded_lines  | [7, 8]
        missing_lines   | []
        num_statements  | 9
        percent_covered | 100.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field           | value
        covered_lines   | 9
        num_statements  | 9
        percent_covered | 100.0
        "#),
        (false, "deferred") => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field          | value
        excluded_lines | [7, 8]
        executed_lines | [2, 4, 5, 10, 14, 17, 19, 21]
        missing_lines  | []

        files.sample.py.summary
        field           | value
        covered_lines   | 8
        excluded_lines  | [7, 8]
        missing_lines   | []
        num_statements  | 8
        percent_covered | 100.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field           | value
        covered_lines   | 8
        num_statements  | 8
        percent_covered | 100.0
        "#),
        (true, "eager") => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | [7, 8]
        executed_branches | [[17, 19], [17, 21]]
        executed_lines    | [2, 4, 5, 10, 12, 14, 17, 19, 21]
        missing_branches  | []
        missing_lines     | []

        files.sample.py.summary
        field                    | value
        covered_branches         | 2
        covered_lines            | 9
        excluded_lines           | [7, 8]
        missing_branches         | 0
        missing_lines            | []
        num_branches             | 2
        num_partial_branches     | 0
        num_statements           | 9
        percent_branches_covered | 100.0
        percent_covered          | 100.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 2
        covered_lines            | 9
        missing_branches         | 0
        num_branches             | 2
        num_partial_branches     | 0
        num_statements           | 9
        percent_branches_covered | 100.0
        percent_covered          | 100.0
        "#),
        _ => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | [7, 8]
        executed_branches | [[17, 19], [17, 21]]
        executed_lines    | [2, 4, 5, 10, 14, 17, 19, 21]
        missing_branches  | []
        missing_lines     | []

        files.sample.py.summary
        field                    | value
        covered_branches         | 2
        covered_lines            | 8
        excluded_lines           | [7, 8]
        missing_branches         | 0
        missing_lines            | []
        num_branches             | 2
        num_partial_branches     | 0
        num_statements           | 8
        percent_branches_covered | 100.0
        percent_covered          | 100.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 2
        covered_lines            | 8
        missing_branches         | 0
        num_branches             | 2
        num_partial_branches     | 0
        num_statements           | 8
        percent_branches_covered | 100.0
        percent_covered          | 100.0
        "#),
    }
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
    insta::assert_snapshot!(report_snapshot(&context.measure("covered()")), @r#"
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

    files.unused.py
    field             | value
    excluded_lines    | []
    executed_branches | []
    executed_lines    | []
    missing_branches  | [[2, 3], [2, 4]]
    missing_lines     | [1, 2, 3, 4]

    files.unused.py.summary
    field                    | value
    covered_branches         | 0
    covered_lines            | 0
    excluded_lines           | []
    missing_branches         | 2
    missing_lines            | [1, 2, 3, 4]
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 4
    percent_branches_covered | 0.0
    percent_covered          | 0.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 0
    covered_lines            | 2
    missing_branches         | 2
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 6
    percent_branches_covered | 0.0
    percent_covered          | 25.0
    "#);
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
    let actual = report_snapshot(&context.measure_with("total()", &config));
    if branches {
        insta::assert_snapshot!(actual, @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | []
    executed_lines    | [1, 2, 6]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 0
    covered_lines            | 3
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 0
    num_partial_branches     | 0
    num_statements           | 3
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 0
    covered_lines            | 3
    missing_branches         | 0
    num_branches             | 0
    num_partial_branches     | 0
    num_statements           | 3
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
    } else {
        insta::assert_snapshot!(actual, @r#"
    files.sample.py
    field          | value
    excluded_lines | []
    executed_lines | [1, 2, 6]
    missing_lines  | []

    files.sample.py.summary
    field           | value
    covered_lines   | 3
    excluded_lines  | []
    missing_lines   | []
    num_statements  | 3
    percent_covered | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field           | value
    covered_lines   | 3
    num_statements  | 3
    percent_covered | 100.0
    "#);
    }
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
    insta::assert_snapshot!(report_snapshot(&context.measure_with(
        r"
            excluded(True)
            partial(True)
            configured(True)
        ",
        &config
    )), @r#"
    files.sample.py
    field             | value
    excluded_lines    | [5, 16]
    executed_branches | [[8, 9]]
    executed_lines    | [1, 2, 3, 7, 8, 9, 12, 13, 14]
    missing_branches  | []
    missing_lines     | [10]

    files.sample.py.summary
    field                    | value
    covered_branches         | 2
    covered_lines            | 9
    excluded_lines           | [5, 16]
    missing_branches         | 0
    missing_lines            | [10]
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 10
    percent_branches_covered | 100.0
    percent_covered          | 91.66666666666666

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 2
    covered_lines            | 9
    missing_branches         | 0
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 10
    percent_branches_covered | 100.0
    percent_covered          | 91.66666666666666
    "#);
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
    insta::assert_snapshot!(report_snapshot(&context.measure_with("choose([1])", &config)), @r#"
    files.sample.py
    field             | value
    excluded_lines    | [6]
    executed_branches | []
    executed_lines    | [1, 2, 3, 7]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 0
    covered_lines            | 4
    excluded_lines           | [6]
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 0
    num_partial_branches     | 0
    num_statements           | 4
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 0
    covered_lines            | 4
    missing_branches         | 0
    num_branches             | 0
    num_partial_branches     | 0
    num_statements           | 4
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
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
    insta::assert_snapshot!(report_snapshot(&context.measure_with("choose(True)", &config)), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[2, 3]]
    executed_lines    | [1, 2, 3]
    missing_branches  | []
    missing_lines     | [4]

    files.sample.py.summary
    field                    | value
    covered_branches         | 2
    covered_lines            | 3
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | [4]
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 4
    percent_branches_covered | 100.0
    percent_covered          | 83.33333333333334

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 2
    covered_lines            | 3
    missing_branches         | 0
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 4
    percent_branches_covered | 100.0
    percent_covered          | 83.33333333333334
    "#);
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
    let actual = error.to_string();
    match kind {
        "exclude" => insta::assert_snapshot!(actual, @"
        ValueError: invalid coverage `exclude-lines` pattern `[`: regex parse error:
            [
            ^
        error: unclosed character class
        "),
        _ => insta::assert_snapshot!(actual, @"
        ValueError: invalid coverage partial-branch pattern `[`: regex parse error:
            [
            ^
        error: unclosed character class
        "),
    }
}
