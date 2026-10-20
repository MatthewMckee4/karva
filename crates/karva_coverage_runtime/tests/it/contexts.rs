use karva_coverage_report::CoverageFilters;
use karva_coverage_runtime::CoveragePhase;

use crate::common::{TestContext, execute, report_snapshot};

#[test]
fn contexts_survive_native_roundtrip_and_filtering() {
    let context = TestContext::new(
        r"
            def choose(flag):
                if flag:
                    return 1
                return 0
        ",
    );
    let mut config = context.config();
    config.contexts = true;
    config.static_context = Some("matrix".to_owned());
    let artifact = context.collect(
        &[
            ("test_true", "choose(True)"),
            ("test_false", "choose(False)"),
        ],
        &config,
    );
    let filters = CoverageFilters::default()
        .with_contexts(&["test_true".to_owned()])
        .expect("context filter");
    insta::assert_snapshot!(report_snapshot(&context.report(&[artifact], &filters)), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[2, 3]]
    executed_lines    | [2, 3]
    missing_branches  | [[2, 4]]
    missing_lines     | [1, 4]

    files.sample.py.contexts
    field | value
    2     | ["matrix|test_true|run"]
    3     | ["matrix|test_true|run"]

    files.sample.py.summary
    field                    | value
    covered_branches         | 1
    covered_lines            | 2
    excluded_lines           | []
    missing_branches         | 1
    missing_lines            | [1, 4]
    num_branches             | 2
    num_partial_branches     | 1
    num_statements           | 4
    percent_branches_covered | 50.0
    percent_covered          | 50.0

    meta
    field         | value
    format        | 2
    show_contexts | true
    version       | "karva"

    totals
    field                    | value
    covered_branches         | 1
    covered_lines            | 2
    missing_branches         | 1
    num_branches             | 2
    num_partial_branches     | 1
    num_statements           | 4
    percent_branches_covered | 50.0
    percent_covered          | 50.0
    "#);
}

#[test]
fn independent_worker_results_combine() {
    let context = TestContext::new(
        r"
            def choose(flag):
                if flag:
                    return 1
                return 0
        ",
    );
    let first = context.collect(&[("first", "choose(True)")], &context.config());
    let second = context.collect(&[("second", "choose(False)")], &context.config());
    insta::assert_snapshot!(report_snapshot(&context.report(&[first, second], &CoverageFilters::default())), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[2, 3], [2, 4]]
    executed_lines    | [1, 2, 3, 4]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 2
    covered_lines            | 4
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 2
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
    covered_branches         | 2
    covered_lines            | 4
    missing_branches         | 0
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 4
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
}

#[test]
fn setup_run_teardown_and_pending_setup() {
    let context = TestContext::new(
        r"
            def setup():
                return 1

            def choose(flag):
                if flag:
                    return 1
                return 0

            def teardown():
                return None
        ",
    );
    let mut config = context.config();
    config.contexts = true;
    config.static_context = Some("environment".to_owned());
    let artifact = context.collect_with(&config, |py, session, globals| {
        session.begin_pending_test_setup(py);
        execute(py, globals, "setup()")?;
        session.resolve_pending_test_setup(py, "test_choose(value)");
        session.set_test_context(py, "test_choose(value)", CoveragePhase::Run);
        execute(py, globals, "choose(True)")?;
        session.set_test_context(py, "test_choose(value)", CoveragePhase::Teardown);
        execute(py, globals, "teardown()")?;
        session.clear_test_context(py);
        execute(py, globals, "choose(False)")
    });
    insta::assert_snapshot!(report_snapshot(&context.report(&[artifact], &CoverageFilters::default())), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[5, 6], [5, 7]]
    executed_lines    | [1, 2, 4, 5, 6, 7, 9, 10]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.contexts
    field | value
    1     | ["environment|session"]
    10    | ["environment|test_choose(value)|teardown"]
    2     | ["environment|test_choose(value)|setup"]
    4     | ["environment|session"]
    5     | ["environment|session", "environment|test_choose(value)|run"]
    6     | ["environment|test_choose(value)|run"]
    7     | ["environment|session"]
    9     | ["environment|session"]

    files.sample.py.summary
    field                    | value
    covered_branches         | 2
    covered_lines            | 8
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 8
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field         | value
    format        | 2
    show_contexts | true
    version       | "karva"

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
    "#);
}
