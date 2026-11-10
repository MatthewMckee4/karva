//! Cross-boundary mutation workflow contracts using real Python worker processes.

#[cfg(unix)]
use std::process::{Command, Output, Stdio};
#[cfg(unix)]
use std::time::Duration;

use indoc::indoc;
use insta_cmd::assert_cmd_snapshot;
#[cfg(unix)]
use karva_mutation::MUTATION_DIR;
use karva_mutation::{MUTATION_RESULTS_PATH, MutationResults, Outcome};

use crate::common::TestContext;

#[cfg(unix)]
fn interrupt_when_started(context: &TestContext, command: &mut Command, marker: &str) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    if let Err(message) =
        crate::cancel::wait_for_file(&context.root().join(marker), Duration::from_secs(20))
    {
        let _ = child.kill();
        let output = child.wait_with_output().expect("wait");
        panic!("{message}: {output:?}");
    }
    let signal = Command::new("kill")
        .args(["-s", "INT", &child.id().to_string()])
        .status()
        .expect("signal");
    assert!(signal.success());
    child.wait_with_output().expect("wait")
}

fn results(context: &TestContext) -> MutationResults {
    MutationResults::read(&context.root().join(MUTATION_RESULTS_PATH)).expect("mutation results")
}

fn project() -> TestContext {
    TestContext::with_files([
        (
            "pyproject.toml",
            "[tool.karva.profile.default.mutation]\nsources = ['app.py']\noperators = ['literal']\nworkers = 2\n",
        ),
        ("app.py", "def enabled():\n    return True\n"),
        (
            "tests/test_app.py",
            "from app import enabled\n\ndef test_enabled():\n    assert enabled() is True\n",
        ),
    ])
}

#[test]
fn kills_mutations_without_changing_source_and_reuses_valid_results() {
    let context = project();
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let first = results(&context);
    let records = first
        .records
        .values()
        .map(|record| (record.outcome, &record.selected_tests))
        .collect::<Vec<_>>();
    insta::assert_debug_snapshot!(records, @r#"
    [
        (
            Killed,
            [
                "tests.test_app::test_enabled",
            ],
        ),
    ]
    "#);
    insta::assert_snapshot!(context.read_file("app.py"), @"
    def enabled():
        return True
    ");

    assert_cmd_snapshot!(context.mutate("run"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 killed app.py:2 enabled
      reused: matching source, tests, dependencies, runtime, environment, and configuration
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    ");

    let cached = results(&context);
    assert!(
        cached
            .records
            .values()
            .all(|record| record.reason.starts_with("reused:"))
    );
}

#[test]
fn inspects_retests_and_applies_a_surviving_mutant() {
    let context = project();
    context.write_file(
        "tests/test_app.py",
        "from app import enabled\n\ndef test_enabled():\n    assert isinstance(enabled(), bool)\n",
    );
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: false
    exit_code: 1
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 survived app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 0
    survived: 1
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let stored = results(&context);
    let id = &stored.records.values().next().expect("record").mutant.id;
    insta::assert_debug_snapshot!(stored.records[id].outcome, @"Survived");

    let mut show = context.mutate("show");
    show.arg(id);
    assert_cmd_snapshot!(show, @"
    success: true
    exit_code: 0
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 survived app.py:2
    duration: [TIME]
    tests: tests.test_app::test_enabled
    full suite: requested or source/test mapping incomplete
    --- a/app.py
    +++ b/app.py
    @@ -1,2 +1,2 @@
     def enabled():
    -    return True
    +    return False

    ----- stderr -----
    ");

    context.write_file(
        "tests/test_app.py",
        "from app import enabled\n\ndef test_enabled():\n    assert enabled() is True\n",
    );

    let mut retest = context.mutate("retest");
    retest.arg(id);
    assert_cmd_snapshot!(retest, @"
    success: true
    exit_code: 0
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    insta::assert_debug_snapshot!(results(&context).records[id].outcome, @"Killed");

    let mut apply = context.mutate("apply");
    apply.arg(id);
    assert_cmd_snapshot!(apply, @"
    success: true
    exit_code: 0
    ----- stdout -----
    Applied 7191b78610ed77ee527de1179b4b50d2 to app.py

    ----- stderr -----
    ");

    insta::assert_snapshot!(context.read_file("app.py"), @"
    def enabled():
        return False
    ");

    let mut stale = context.mutate("apply");
    stale.arg(id);
    assert_cmd_snapshot!(stale, @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: mutant 7191b78610ed77ee527de1179b4b50d2: stale source `app.py`; rerun `uv run karva mutate run` before applying
    ");

    insta::assert_snapshot!(context.read_file("app.py"), @"
    def enabled():
        return False
    ");
}

#[test]
fn baseline_failure_preserves_original_failures_and_does_not_create_results() {
    let context = project();
    context.write_file(
        "tests/test_app.py",
        "def test_baseline():\n    assert False, 'baseline evidence'\n",
    );
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: false
    exit_code: 2
    ----- stdout -----

    failures:

    tests.test_app::test_baseline:

    error[test-failure]: Test `test_baseline` failed
     --> tests/test_app.py:1:5
      |
    1 | def test_baseline():
      |     ^^^^^^^^^^^^^
    info: Test failed here
     --> tests/test_app.py:2:5
      |
    2 |     assert False, 'baseline evidence'
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: baseline evidence

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    karva failed
      Cause: mutation baseline failed; fix the unmodified test suite before running mutants
    ");
    assert!(!context.root().join(MUTATION_RESULTS_PATH).exists());
}

#[test]
fn reports_uncovered_sources_and_rejects_invalid_filters_and_budgets() {
    let context = project();
    context.write_file("tests/test_app.py", "def test_other():\n    pass\n");
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: false
    exit_code: 1
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 uncovered app.py:2 enabled
      uncovered: source was not imported; full suite safety check passed
    killed: 0
    survived: 0
    uncovered: 1
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");
    assert_eq!(
        results(&context)
            .records
            .values()
            .next()
            .expect("record")
            .outcome,
        Outcome::Uncovered
    );
    let mut invalid = context.mutate("run");
    invalid.args(["--operator", "typo"]);
    assert_cmd_snapshot!(invalid, @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: unknown mutation operator `typo`; allowed: literal, comparison, boolean, arithmetic, control-flow, return
    ");

    let mut invalid = context.mutate("run");
    invalid.args(["--timeout", "0"]);
    assert_cmd_snapshot!(invalid, @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: mutation timeout budget requested 0; allowed values are positive finite seconds representable on this platform
    ");
}

#[test]
fn json_and_terminal_reports_use_the_same_stored_outcomes_from_nested_directories() {
    let context = project();
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let mut nested = context.karva_command_in(context.root().join("tests"));
    nested.args(["mutate", "results"]);
    assert_cmd_snapshot!(nested, @"
    success: true
    exit_code: 0
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    ");

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r#"("context_fingerprint": ")[0-9a-f]+""#, "$1[CONTEXT]\"");
    settings.add_filter(r#"("baseline_seconds": )\d+(?:\.\d+)?"#, "$1\"[TIME]\"");
    settings.add_filter(r#"(?m)^(\s*"diagnostic": ).*$"#, "$1\"[DIAGNOSTIC]\"");
    let _guard = settings.bind_to_scope();
    let mut json = context.mutate("results");
    json.arg("--json");
    assert_cmd_snapshot!(json, @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    {
      "format_version": 1,
      "context_fingerprint": "[CONTEXT]",
      "configuration": {
        "mutation": {
          "exclude": null,
          "exclude-lines": null,
          "exclude-symbols": null,
          "operators": [
            "literal"
          ],
          "sources": [
            "app.py"
          ],
          "timeout": null,
          "workers": 2
        },
        "profile": null,
        "sources": [
          "app.py"
        ],
        "timeout_seconds": null,
        "workers": 2
      },
      "baseline_seconds": "[TIME]",
      "records": {
        "7191b78610ed77ee527de1179b4b50d2": {
          "mutant": {
            "id": "7191b78610ed77ee527de1179b4b50d2",
            "path": "app.py",
            "source_fingerprint": "89cb0d534f9c879d7708e7c2106ee67a",
            "start": 26,
            "end": 30,
            "line": 2,
            "symbol": "enabled",
            "operator": "literal",
            "original": "True",
            "replacement": "False"
          },
          "context_fingerprint": "[CONTEXT]",
          "outcome": "killed",
          "selected_tests": [
            "tests.test_app::test_enabled"
          ],
          "duration_seconds": "[TIME]",
          "reason": "full suite: requested or source/test mapping incomplete",
          "diagnostic": "[DIAGNOSTIC]"
        }
      }
    }

    ----- stderr -----
    "#);
}

#[test]
fn affected_selection_matches_full_suite_and_dependency_changes_invalidate_results() {
    let context = TestContext::with_files([
        (
            "pyproject.toml",
            "[tool.karva.profile.default.mutation]\nsources = ['app.py']\noperators = ['literal']\nworkers = 1\n",
        ),
        ("app.py", "def enabled():\n    return True\n"),
        (
            "tests/test_app.py",
            indoc! {r"
            from app import enabled

            def test_enabled():
                assert enabled() is True

            def test_unrelated():
                pass
        "},
        ),
        ("uv.lock", "dependency version one"),
    ]);
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 killed app.py:2 enabled
      affected tests first; full-suite safety check before survival
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let optimized = results(&context);
    let record = optimized.records.values().next().expect("record");
    assert!(record.reason.starts_with("affected tests first"));
    assert_eq!(record.selected_tests, ["tests.test_app::test_enabled"]);
    let mut full = context.mutate("run");
    full.args(["--full-suite", "--no-cache"]);
    assert_cmd_snapshot!(full, @"
    success: true
    exit_code: 0
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let full = results(&context);
    assert_eq!(record.outcome, full.records[&record.mutant.id].outcome);
    context.write_file("uv.lock", "dependency version two");
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 killed app.py:2 enabled
      affected tests first; full-suite safety check before survival
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");
    assert!(
        results(&context)
            .records
            .values()
            .all(|record| !record.reason.starts_with("reused:"))
    );
}

#[test]
fn concurrent_mutants_use_independent_interpreters_and_original_source() {
    let context = project();
    let source = "counter = 0 # pragma: no mutate\ndef flags():\n    global counter\n    counter += 1 # pragma: no mutate\n    assert counter == 1 # pragma: no mutate\n    return (True, False)\n";
    context.write_file("app.py", source);
    context.write_file(
        "tests/test_app.py",
        indoc! {r"
        from pathlib import Path
        from app import flags

        def test_flags():
            assert 'return (True, False)' in Path('app.py').read_text()
            assert flags() == (True, False)
    "},
    );
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    591e6fa2a8df047794f37aeda1113dda killed app.py:6 flags
      full suite: requested or source/test mapping incomplete
    f97617e0d60825bc9e128ae591e4d964 killed app.py:6 flags
      full suite: requested or source/test mapping incomplete
    killed: 2
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/2
    Completed mutation 2/2
    ");

    let stored = results(&context);
    let outcomes = stored
        .records
        .values()
        .map(|record| record.outcome)
        .collect::<Vec<_>>();
    insta::assert_debug_snapshot!(outcomes, @"
    [
        Killed,
        Killed,
    ]
    ");
    assert_eq!(context.read_file("app.py"), source);
}

#[test]
fn invalid_compile_candidates_do_not_execute_tests() {
    let context = project();
    context.write_file(
        "app.py",
        indoc! {r"
        def combine(mapping):
            match mapping:
                case {True: left, False: right}:
                    return left + right
    "},
    );
    context.write_file("tests/test_app.py", "from app import combine\n\ndef test_combine():\n    assert combine({True: 1, False: 2}) == 3\n");
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    46d8b80b2b2d2c5895e385da0aac97c3 invalid app.py:3 combine
      full suite: requested or source/test mapping incomplete
    f9e34f7848ba77686214e6ec42a96bf2 invalid app.py:3 combine
      full suite: requested or source/test mapping incomplete
    killed: 0
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 2
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/2
    Completed mutation 2/2
    ");

    let stored = results(&context);
    let records = stored
        .records
        .values()
        .map(|record| (record.outcome, &record.selected_tests))
        .collect::<Vec<_>>();
    insta::assert_debug_snapshot!(records, @"
    [
        (
            Invalid,
            [],
        ),
        (
            Invalid,
            [],
        ),
    ]
    ");
}

#[test]
fn measured_timeout_diagnostics_name_the_mutant_and_budget() {
    let context = project();
    context.write_file("app.py", "def spin():\n    while False:\n        pass\n");
    context.write_file(
        "tests/test_app.py",
        "from app import spin\n\ndef test_spin():\n    spin()\n",
    );

    let mut run = context.mutate("run");
    run.args(["--timeout", "1"]);
    assert_cmd_snapshot!(run, @"
    success: false
    exit_code: 1
    ----- stdout -----
    854f2349eab34db4ebc3a95e054511ba timed-out app.py:2 spin
      full suite: requested or source/test mapping incomplete
      mutant 854f2349eab34db4ebc3a95e054511ba: mutation-timeout budget exceeded; limit [TIME], requested 1 tests (tests.test_app::test_spin), measured baseline [TIME]
    killed: 0
    survived: 0
    uncovered: 0
    timed-out: 1
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let stored = results(&context);
    let record = stored.records.values().next().expect("record");
    insta::assert_debug_snapshot!((record.outcome, &record.selected_tests), @r#"
    (
        TimedOut,
        [
            "tests.test_app::test_spin",
        ],
    )
    "#);

    let diagnostic = record.diagnostic.as_ref().expect("timeout diagnostic");
    assert!(diagnostic.contains(&record.mutant.id));
    assert!(diagnostic.contains("limit 1.000000s"));
    assert!(diagnostic.contains("measured baseline"));
    insta::assert_snapshot!(context.read_file("app.py"), @"
    def spin():
        while False:
            pass
    ");
}

#[test]
fn worker_crashes_are_infrastructure_errors_instead_of_killed_mutants() {
    let context = project();
    context.write_file(
        "app.py",
        "import os\n\ndef maybe_exit():\n    if False:\n        os._exit(7) # pragma: no mutate\n",
    );
    context.write_file(
        "tests/test_app.py",
        "from app import maybe_exit\n\ndef test_exit():\n    maybe_exit()\n",
    );
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: false
    exit_code: 2
    ----- stdout -----
    dac2e9e551d8b4a0af5c8b091ebae040 errored app.py:4 maybe_exit
      full suite: requested or source/test mapping incomplete
      mutant dac2e9e551d8b4a0af5c8b091ebae040: worker execution failed; requested 1 tests (tests.test_app::test_exit), measured baseline [TIME], budget [TIME]
    killed: 0
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 1
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let stored = results(&context);
    let record = stored.records.values().next().expect("record");
    assert_eq!(record.outcome, Outcome::Errored);
    let diagnostic = record.diagnostic.as_ref().expect("worker diagnostic");
    assert!(diagnostic.contains(&record.mutant.id));
    assert!(diagnostic.contains("requested"));
    assert!(diagnostic.contains("measured baseline"));
}

#[cfg(unix)]
#[test]
fn cancellation_preserves_completed_results_and_resumes_pending_mutants() {
    let context = project();
    let marker = format!("{MUTATION_DIR}/started");

    context.write_file(
        "app.py",
        "def first():\n    return True\n\ndef second():\n    return False\n",
    );
    context.write_file(
        "tests/test_app.py",
        format!(
            indoc! {r"
        from pathlib import Path
        import time
        from app import first, second

        def test_flags():
            assert first() is True
            if second():
                Path({marker:?}).touch()
                while True:
                    time.sleep(1)
    "},
            marker = marker
        ),
    );

    let mut run = context.mutate("run");
    run.args(["--workers", "1", "--timeout", "1"]);
    let output = interrupt_when_started(&context, &mut run, &marker);
    insta::assert_snapshot!(format!("exit_code: {:?}\n----- stdout -----\n{}\n----- stderr -----\n{}", output.status.code(), String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)), @"
    exit_code: Some(2)
    ----- stdout -----
    1eb2d4c0d619cf1bd2aa1f35d025cfed pending app.py:5 second
      full suite: requested or source/test mapping incomplete
    583db14762f166d5873b145460c0be3b killed app.py:2 first
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 1

    ----- stderr -----
    Completed mutation 1/2
    Completed mutation 2/2
    ");

    let interrupted = results(&context);
    assert_eq!(
        interrupted
            .records
            .values()
            .filter(|record| record.outcome == Outcome::Killed)
            .count(),
        1
    );
    assert_eq!(
        interrupted
            .records
            .values()
            .filter(|record| record.outcome == Outcome::Pending)
            .count(),
        1
    );
    let mut resumed = context.mutate("run");
    resumed.args(["--workers", "1", "--timeout", "1"]);
    assert_cmd_snapshot!(resumed, @"
    success: false
    exit_code: 1
    ----- stdout -----
    1eb2d4c0d619cf1bd2aa1f35d025cfed timed-out app.py:5 second
      full suite: requested or source/test mapping incomplete
      mutant 1eb2d4c0d619cf1bd2aa1f35d025cfed: mutation-timeout budget exceeded; limit [TIME], requested 1 tests (tests.test_app::test_flags), measured baseline [TIME]
    583db14762f166d5873b145460c0be3b killed app.py:2 first
      reused: matching source, tests, dependencies, runtime, environment, and configuration
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 1
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let resumed = results(&context);
    assert!(resumed.records.values().any(|record| record.outcome == Outcome::Killed && record.reason.starts_with("reused:")));
    assert!(
        resumed
            .records
            .values()
            .any(|record| record.outcome == Outcome::TimedOut)
    );
}

#[test]
fn installed_dependency_source_changes_invalidate_cached_results() {
    let context = project();
    let dependency = tempfile::tempdir().expect("dependency directory");
    let metadata = dependency.path().join("mutation_probe-1.0.dist-info");
    std::fs::create_dir_all(&metadata).expect("metadata directory");
    std::fs::write(
        metadata.join("METADATA"),
        "Metadata-Version: 2.1\nName: mutation-probe\nVersion: 1.0\n",
    )
    .expect("metadata");
    std::fs::write(metadata.join("RECORD"), "mutation_probe.py,,\n").expect("record");
    let module = dependency.path().join("mutation_probe.py");
    std::fs::write(&module, "flag = True\n").expect("dependency");
    context.write_file("tests/test_app.py", "from app import enabled\nimport mutation_probe\n\ndef test_enabled():\n    assert isinstance(mutation_probe.flag, bool)\n    assert enabled() is True\n");
    let execute = || {
        let mut run = context.mutate("run");
        run.env("PYTHONPATH", dependency.path());
        run
    };
    assert_cmd_snapshot!(execute(), @"
    success: true
    exit_code: 0
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let first = results(&context);
    std::fs::write(&module, "flag = False\n").expect("changed dependency");
    assert_cmd_snapshot!(execute(), @"
    success: true
    exit_code: 0
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let second = results(&context);
    assert_ne!(first.context_fingerprint, second.context_fingerprint);
    assert!(
        second
            .records
            .values()
            .all(|record| !record.reason.starts_with("reused:"))
    );
}

#[test]
fn retesting_one_candidate_preserves_unrelated_results_and_marks_them_stale() {
    let context = project();
    context.write_file("app.py", "def flags():\n    return True, False\n");
    context.write_file("tests/test_app.py", "from app import flags\n\ndef test_flags():\n    assert all(isinstance(value, bool) for value in flags())\n");
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: false
    exit_code: 1
    ----- stdout -----
    ea9967fd18b48b836f129e570f04b105 survived app.py:2 flags
      full suite: requested or source/test mapping incomplete
    feb84bf9b609d4f18a46f01084e74afe survived app.py:2 flags
      full suite: requested or source/test mapping incomplete
    killed: 0
    survived: 2
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/2
    Completed mutation 2/2
    ");

    let first = results(&context);
    let selected = first
        .records
        .values()
        .find(|record| record.mutant.original == "True")
        .expect("first");
    let unrelated = first
        .records
        .values()
        .find(|record| record.mutant.original == "False")
        .expect("second");
    context.write_file(
        "tests/test_app.py",
        "from app import flags\n\ndef test_flags():\n    assert flags()[0] is True\n",
    );

    let mut retest = context.mutate("retest");
    retest.arg(&selected.mutant.id);
    assert_cmd_snapshot!(retest, @"
    success: true
    exit_code: 0
    ----- stdout -----
    feb84bf9b609d4f18a46f01084e74afe killed app.py:2 flags
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let second = results(&context);
    assert_eq!(second.records[&selected.mutant.id].outcome, Outcome::Killed);
    assert_eq!(
        second.records[&unrelated.mutant.id].duration_seconds,
        unrelated.duration_seconds
    );
    assert_eq!(
        second.records[&unrelated.mutant.id].context_fingerprint,
        unrelated.context_fingerprint
    );
    assert_ne!(
        second.records[&unrelated.mutant.id].context_fingerprint,
        second.context_fingerprint
    );
    assert_cmd_snapshot!(context.mutate("results"), @"
    success: false
    exit_code: 2
    ----- stdout -----
    ea9967fd18b48b836f129e570f04b105 survived app.py:2 flags
      full suite: requested or source/test mapping incomplete
      stale: execution context changed; retest required
    feb84bf9b609d4f18a46f01084e74afe killed app.py:2 flags
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 1
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    ");
}

#[test]
fn rediscovery_removes_edited_and_deleted_source_records_before_bulk_retesting() {
    let context = project();
    context.write_file("pyproject.toml", "[tool.karva.profile.default.mutation]\nsources = ['.']\noperators = ['literal']\nworkers = 1\n");
    context.write_file("obsolete.py", "value = False\n");
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: false
    exit_code: 1
    ----- stdout -----
    111bd982bfafade21213f8ee02bce1c7 uncovered obsolete.py:1
      uncovered: source was not imported; full suite safety check passed
    7191b78610ed77ee527de1179b4b50d2 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 1
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/2
    Completed mutation 2/2
    ");

    let first = results(&context);
    assert_eq!(first.records.len(), 2);
    context.write_file(
        "app.py",
        "def enabled():\n    return True # updated source\n",
    );
    std::fs::remove_file(context.root().join("obsolete.py")).expect("delete source");
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    2b55d4c855448f4e4a609f43e142e474 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let current = results(&context);
    assert_eq!(current.records.len(), 1);
    assert!(
        current
            .records
            .keys()
            .all(|id| !first.records.contains_key(id))
    );
    assert_cmd_snapshot!(context.mutate("results"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    2b55d4c855448f4e4a609f43e142e474 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.mutate("retest"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    2b55d4c855448f4e4a609f43e142e474 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");
    assert_eq!(results(&context).records.len(), 1);
}

#[test]
fn bulk_retesting_skips_stale_records_and_rejects_discovery_only_flags() {
    let context = project();
    context.write_file("app.py", "def enabled():\n    return True\n");
    context.write_file("other.py", "value = True\n");
    context.write_file("pyproject.toml", "[tool.karva.profile.default.mutation]\nsources = ['.']\noperators = ['literal']\nworkers = 1\n");
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: false
    exit_code: 1
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    8368b42eb8074dc5afe5ea7e6a027537 uncovered other.py:1
      uncovered: source was not imported; full suite safety check passed
    killed: 1
    survived: 0
    uncovered: 1
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/2
    Completed mutation 2/2
    ");
    std::fs::remove_file(context.root().join("other.py")).expect("delete source");
    assert_cmd_snapshot!(context.mutate("retest"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    7191b78610ed77ee527de1179b4b50d2 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");
    assert_eq!(results(&context).records.len(), 1);
    let mut retest = context.mutate("retest");
    retest.args(["--source", "app.py"]);
    assert_cmd_snapshot!(retest, @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: `--source` and `--exclude` apply only to `mutate run`; use `--path` to select recorded candidates for retesting
    ");
}

#[test]
fn utf8_bom_source_mutations_compile_and_preserve_the_bom() {
    let context = project();
    let source = "\u{feff}def enabled():\n    return True\n";
    context.write_file("app.py", source);
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    9610700bbb8f6f8fd46bfdb8055353a7 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let stored = results(&context);
    assert_eq!(stored.records.len(), 1);
    assert_eq!(
        stored.records.values().next().expect("record").outcome,
        Outcome::Killed
    );
    assert_eq!(context.read_file("app.py"), source);
}

#[test]
fn default_timeout_uses_the_measured_baseline_without_multiplying_worker_count() {
    let context = project();
    context.write_file(
        "app.py",
        "def spin():\n    while False:\n        pass\n    return True\n",
    );
    context.write_file(
        "tests/test_app.py",
        "from app import spin\n\ndef test_spin():\n    assert spin() is True\n",
    );
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: false
    exit_code: 1
    ----- stdout -----
    26ce20e81fba0c7294d9e9aa592e0a05 killed app.py:4 spin
      full suite: requested or source/test mapping incomplete
    6b065d91b2e6b7103d205bfe2b5397ed timed-out app.py:2 spin
      full suite: requested or source/test mapping incomplete
      mutant 6b065d91b2e6b7103d205bfe2b5397ed: mutation-timeout budget exceeded; limit [TIME], requested 1 tests (tests.test_app::test_spin), measured baseline [TIME]
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 1
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/2
    Completed mutation 2/2
    ");

    let stored = results(&context);
    assert_eq!(stored.records.len(), 2);
    let timeout = stored
        .records
        .values()
        .find(|record| record.outcome == Outcome::TimedOut)
        .expect("timeout");
    let expected = format!("limit {:.6}s", stored.baseline_seconds * 10.0);
    assert!(
        timeout
            .diagnostic
            .as_ref()
            .expect("diagnostic")
            .contains(&expected)
    );
}

#[cfg(unix)]
#[test]
fn baseline_cancellation_does_not_report_a_test_failure() {
    let context = project();
    let marker = format!("{MUTATION_DIR}/baseline-started");

    context.write_file("tests/test_app.py", format!("from pathlib import Path\nimport time\n\ndef test_baseline():\n    Path({marker:?}).touch()\n    time.sleep(60)\n"));

    let output = interrupt_when_started(&context, &mut context.mutate("run"), &marker);
    insta::assert_snapshot!(format!("exit_code: {:?}\n----- stdout -----\n{}\n----- stderr -----\n{}", output.status.code(), String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)), @"
    exit_code: Some(2)
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: mutation baseline cancelled; no mutants were executed
    ");
    assert!(!context.root().join(MUTATION_RESULTS_PATH).exists());
}

fn project_with_multiple_operators() -> TestContext {
    TestContext::with_files([
        (
            "pyproject.toml",
            "[tool.karva.profile.default.mutation]\nsources = ['.']\noperators = ['literal', 'arithmetic', 'comparison', 'return']\nworkers = 1\n",
        ),
        ("app.py", "def enabled(value):\n    return value + 1 > 0\n"),
        ("other.py", "def other():\n    return True\n"),
        (
            "tests/test_app.py",
            "from app import enabled\nfrom other import other\n\ndef test_enabled():\n    assert enabled(1) is True\n    assert enabled(-1) is False\n    assert enabled(0) is True\n    assert other() is True\n",
        ),
    ])
}

#[test]
fn narrowed_runs_preserve_unselected_operator_and_file_results() {
    let context = project_with_multiple_operators();
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    2a224aa779d2868d43c412b445aa0cc5 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    3a42a5cd8c610abd85544e06bdc75708 killed other.py:2 other
      full suite: requested or source/test mapping incomplete
    7d79666a6a6effe03e0269bbb9949a06 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    80a9e8299a1b9a7bd91d6a3d016d6e26 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    8b9b70b76b7a77d2029c226a51d93ab1 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    b720be33d49680ad937c86bd5481b1c7 killed other.py:2 other
      full suite: requested or source/test mapping incomplete
    e8816f64edea6efd00732e3d386aa8a1 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 7
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/7
    Completed mutation 2/7
    Completed mutation 3/7
    Completed mutation 4/7
    Completed mutation 5/7
    Completed mutation 6/7
    Completed mutation 7/7
    ");

    let first = results(&context);
    assert_eq!(first.records.len(), 7);
    let mut comparison = context.mutate("run");
    comparison.args(["--operator", "comparison"]);
    assert_cmd_snapshot!(comparison, @"
    success: true
    exit_code: 0
    ----- stdout -----
    7d79666a6a6effe03e0269bbb9949a06 killed app.py:2 enabled
      reused: matching source, tests, dependencies, runtime, environment, and configuration
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    ");

    let narrowed = results(&context);
    assert_eq!(narrowed.records.len(), first.records.len());
    assert!(
        first
            .records
            .keys()
            .all(|id| narrowed.records.contains_key(id))
    );
    assert_eq!(first.context_fingerprint, narrowed.context_fingerprint);

    let mut excluded = context.mutate("run");
    excluded.args(["--exclude", "app.py"]);
    assert_cmd_snapshot!(excluded, @"
    success: true
    exit_code: 0
    ----- stdout -----
    3a42a5cd8c610abd85544e06bdc75708 killed other.py:2 other
      reused: matching source, tests, dependencies, runtime, environment, and configuration
    b720be33d49680ad937c86bd5481b1c7 killed other.py:2 other
      reused: matching source, tests, dependencies, runtime, environment, and configuration
    killed: 2
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    ");

    let excluded = results(&context);
    assert_eq!(excluded.records.len(), first.records.len());
    assert!(
        first
            .records
            .keys()
            .all(|id| excluded.records.contains_key(id))
    );
    assert_eq!(first.context_fingerprint, excluded.context_fingerprint);

    assert_cmd_snapshot!(context.mutate("results"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    2a224aa779d2868d43c412b445aa0cc5 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    3a42a5cd8c610abd85544e06bdc75708 killed other.py:2 other
      reused: matching source, tests, dependencies, runtime, environment, and configuration
    7d79666a6a6effe03e0269bbb9949a06 killed app.py:2 enabled
      reused: matching source, tests, dependencies, runtime, environment, and configuration
    80a9e8299a1b9a7bd91d6a3d016d6e26 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    8b9b70b76b7a77d2029c226a51d93ab1 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    b720be33d49680ad937c86bd5481b1c7 killed other.py:2 other
      reused: matching source, tests, dependencies, runtime, environment, and configuration
    e8816f64edea6efd00732e3d386aa8a1 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 7
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    ");
}

#[test]
fn narrowed_runs_remove_changed_source_records() {
    let context = project_with_multiple_operators();
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    2a224aa779d2868d43c412b445aa0cc5 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    3a42a5cd8c610abd85544e06bdc75708 killed other.py:2 other
      full suite: requested or source/test mapping incomplete
    7d79666a6a6effe03e0269bbb9949a06 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    80a9e8299a1b9a7bd91d6a3d016d6e26 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    8b9b70b76b7a77d2029c226a51d93ab1 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    b720be33d49680ad937c86bd5481b1c7 killed other.py:2 other
      full suite: requested or source/test mapping incomplete
    e8816f64edea6efd00732e3d386aa8a1 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 7
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/7
    Completed mutation 2/7
    Completed mutation 3/7
    Completed mutation 4/7
    Completed mutation 5/7
    Completed mutation 6/7
    Completed mutation 7/7
    ");

    let first = results(&context);
    context.write_file(
        "app.py",
        "# moved\ndef enabled(value):\n    return value + 1 > 0\n",
    );

    let mut narrowed = context.mutate("run");
    narrowed.args(["--operator", "comparison"]);
    assert_cmd_snapshot!(narrowed, @"
    success: true
    exit_code: 0
    ----- stdout -----
    c37fdc64061b17f9a926c65e933540e0 killed app.py:3 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/1
    ");

    let updated = results(&context);
    assert_eq!(updated.records.len(), 3);
    for (id, record) in &first.records {
        assert_eq!(
            updated.records.contains_key(id),
            record.mutant.path == "other.py"
        );
    }

    let mut report = context.mutate("results");
    report.args(["--path", "app.py"]);
    assert_cmd_snapshot!(report, @"
    success: true
    exit_code: 0
    ----- stdout -----
    c37fdc64061b17f9a926c65e933540e0 killed app.py:3 enabled
      full suite: requested or source/test mapping incomplete
    killed: 1
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    ");
}

#[rstest::rstest]
fn empty_narrowed_runs_leave_stored_results_unchanged(
    #[values("--operator=boolean", "--exclude=*.py")] argument: &str,
) {
    insta::allow_duplicates! {
    let context = project_with_multiple_operators();
    assert_cmd_snapshot!(context.mutate("run"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    2a224aa779d2868d43c412b445aa0cc5 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    3a42a5cd8c610abd85544e06bdc75708 killed other.py:2 other
      full suite: requested or source/test mapping incomplete
    7d79666a6a6effe03e0269bbb9949a06 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    80a9e8299a1b9a7bd91d6a3d016d6e26 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    8b9b70b76b7a77d2029c226a51d93ab1 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    b720be33d49680ad937c86bd5481b1c7 killed other.py:2 other
      full suite: requested or source/test mapping incomplete
    e8816f64edea6efd00732e3d386aa8a1 killed app.py:2 enabled
      full suite: requested or source/test mapping incomplete
    killed: 7
    survived: 0
    uncovered: 0
    timed-out: 0
    invalid: 0
    errored: 0
    pending: 0

    ----- stderr -----
    Completed mutation 1/7
    Completed mutation 2/7
    Completed mutation 3/7
    Completed mutation 4/7
    Completed mutation 5/7
    Completed mutation 6/7
    Completed mutation 7/7
    ");

    let original = context.read_file(MUTATION_RESULTS_PATH);

    let mut narrowed = context.mutate("run");
    narrowed.arg(argument);
    assert_cmd_snapshot!(narrowed, @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: no mutations match the supplied sources and filters; check mutation.sources, --source, --path, --symbol, --operator, and --outcome
    ");
    assert_eq!(context.read_file(MUTATION_RESULTS_PATH), original);
    }
}
