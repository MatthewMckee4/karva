use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;
use serde_json::{Value, json};

use crate::common::TestContext;

#[test]
fn lifecycle_seeds_repeat_across_retries() {
    let context = TestContext::with_file(
        "test_random.py",
        r#"
        import os
        import random

        import karva

        def record(phase):
            with open("random.log", "a", encoding="utf-8") as output:
                output.write(f"{phase}:{os.environ['KARVA_RANDOM_SEED']}:{random.random()}\n")

        @karva.fixture
        def seeded_fixture():
            record("setup")
            yield
            record("teardown")

        def test_random(seeded_fixture):
            record("call")
            random.seed(17)
            assert random.random() == 0.5219839097124932
            assert os.environ["KARVA_ATTEMPT"] == "2"
        "#,
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--random-seed=170938",
            "--retry=1",
            "--status-level=none",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test_random::test_random(seeded_fixture=None)

    ----- stderr -----
    "
    );
    assert_snapshot!(context.read_file("random.log"), @"
    setup:7516306709688983817:0.2703726869595934
    call:7421003177346115495:0.5671970256751211
    teardown:9017368182005858673:0.7917082330962069
    setup:7516306709688983817:0.2703726869595934
    call:7421003177346115495:0.5671970256751211
    teardown:9017368182005858673:0.7917082330962069
    ");
}

#[test]
fn seeds_do_not_depend_on_scheduling_or_selection() {
    let context = TestContext::with_file(
        "test_random.py",
        r#"
        import os
        import random

        def test_keep():
            with open("observed.txt", "w", encoding="utf-8") as output:
                output.write(f"{os.environ['KARVA_RANDOM_SEED']}:{random.random()}")

        def test_noise_1(): pass
        def test_noise_2(): pass
        def test_noise_3(): pass
        def test_noise_4(): pass
        def test_noise_5(): pass
        def test_noise_6(): pass
        def test_noise_7(): pass
        def test_noise_8(): pass
        def test_noise_9(): pass
        "#,
    );

    let serial = run_observed(&context, &["--no-parallel"]);
    let parallel = run_observed(&context, &["--num-workers=2", "--shuffle"]);
    let filtered = run_observed(&context, &["--no-parallel", "--filter=test(~keep)"]);
    let partitions = [
        run_observed(&context, &["--no-parallel", "--partition=slice:1/2"]),
        run_observed(&context, &["--no-parallel", "--partition=slice:2/2"]),
    ];

    assert_eq!(parallel, serial);
    assert_eq!(filtered, serial);
    assert_eq!(
        partitions.iter().find(|value| !value.is_empty()),
        Some(&serial)
    );
}

fn run_observed(context: &TestContext, args: &[&str]) -> String {
    context.write_file("observed.txt", "");
    let mut command = context.command();
    command
        .args(["--random-seed=170938", "--status-level=none"])
        .args(args);
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"\d+ tests? run: \d+ passed", "[TESTS]");
    settings.add_filter(r"\d+ skipped", "[SKIPPED]");
    insta::allow_duplicates! {
        settings.bind(|| assert_cmd_snapshot!(command, @"
        success: true
        exit_code: 0
        ----- stdout -----
        Random seed: [SEED]
        ────────────
             Summary [TIME] [TESTS], [SKIPPED]

        ----- stderr -----
        "));
    }
    context.read_file("observed.txt")
}

#[test]
fn parametrized_variants_receive_distinct_seeds() {
    let context = TestContext::with_file(
        "test_random.py",
        r#"
        import os

        import karva

        @karva.tags.parametrize("value", ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa2"])
        def test_random(value):
            with open(f"seed-{value[-1]}.txt", "w", encoding="utf-8") as output:
                output.write(os.environ["KARVA_RANDOM_SEED"])
        "#,
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--random-seed=170938", "--status-level=none"]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    "
    );

    assert_ne!(
        context.read_file("seed-1.txt"),
        context.read_file("seed-2.txt")
    );
}

#[test]
fn failure_diagnostics_include_seeds() {
    let context = TestContext::with_file(
        "test_random.py",
        r"
        import random

        def test_failure():
            assert random.random() < 0
        ",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--random-seed=170938", "--status-level=none"]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
    Random seed: [SEED]

    failures:

    test_random::test_failure:

    error[test-failure]: Test `test_failure` failed
     --> test_random.py:4:5
      |
    4 | def test_failure():
      |     ^^^^^^^^^^^^
      |
    info: Test failed here
     --> test_random.py:5:5
      |
    5 |     assert random.random() < 0
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^
      |

    Random seed: [SEED]
    Phase seeds: setup=6637454972422594998, call=2091077975967582004, teardown=15821255015519234932

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );
}

#[rstest]
fn reports_include_seeds(
    #[values(("results.json", None), ("results.jsonl", Some("jsonl")))] report: (
        &str,
        Option<&str>,
    ),
) {
    let context = TestContext::with_file("test_random.py", "def test_random(): pass");
    let (path, format) = report;
    let mut command = context.command_no_parallel();
    command
        .args(["--random-seed=170938", "--status-level=none"])
        .arg(format!("--result-output={path}"));
    if let Some(format) = format {
        command.arg(format!("--result-format={format}"));
    }

    insta::allow_duplicates! {
        assert_cmd_snapshot!(command, @"
        success: true
        exit_code: 0
        ----- stdout -----
        Random seed: [SEED]
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");

        let contents = context.read_file(path);
        let report = if format.is_some() {
            let records = contents
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).expect("parse JSONL record"))
                .collect::<Vec<_>>();
            let test = records
                .iter()
                .find(|record| record["type"] == "test")
                .expect("test record");
            let finished = records
                .iter()
                .find(|record| record["type"] == "run_finished")
                .expect("run_finished record");
            json!({
                "random_seed": finished["random_seed"],
                "random_seeds": test["random_seeds"],
            })
        } else {
            let document = serde_json::from_str::<Value>(&contents).expect("parse JSON report");
            json!({
                "random_seed": document["random_seed"],
                "random_seeds": document["tests"][0]["random_seeds"],
            })
        };
        assert_snapshot!(
            serde_json::to_string_pretty(&report).expect("serialize report seeds"),
            @r#"
        {
          "random_seed": 170938,
          "random_seeds": {
            "base": 170938,
            "call": 7421003177346115495,
            "setup": 7516306709688983817,
            "teardown": 9017368182005858673
          }
        }
        "#
        );
    }
}
