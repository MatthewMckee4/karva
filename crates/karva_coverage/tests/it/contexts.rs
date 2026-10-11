use karva_coverage::{CoverageFilters, CoveragePhase};

use crate::common::{TestContext, execute};

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
    insta::assert_json_snapshot!(context.report(&[artifact], &filters));
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
    insta::assert_json_snapshot!(context.report(&[first, second], &CoverageFilters::default()));
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
    insta::assert_json_snapshot!(context.report(&[artifact], &CoverageFilters::default()));
}
