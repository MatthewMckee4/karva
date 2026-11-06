use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;
use serde_json::Value;

use crate::common::TestContext;

#[test]
fn validates_sampling_controls_and_callable_contracts() {
    let context = TestContext::with_file(
        "test_contract.py",
        r#"
import karva
import karva._benchmark as sampling
import pytest

@karva.tags.parametrize("options, message", [
    ({"iterations": -1}, "iterations must be an integer >= 1; requested -1"),
    ({"warmup": -1}, "warmup must be an integer >= 0; requested -1"),
    ({"min_time": 0}, "min_time must be finite and > 0 seconds; requested 0"),
    ({"max_time": float("inf")}, "max_time must be finite and > 0 seconds; requested inf"),
    ({"min_time": 2, "max_time": 1}, "min_time requested 2s exceeds max_time 1s"),
    ({"unknown": True}, "unknown benchmark option: unknown"),
    ({"group": 1}, "group must be a string or None"),
    ({"disable_gc": 1}, "disable_gc must be a boolean"),
])
def test_validation(options, message):
    with pytest.raises((ValueError, TypeError), match=message):
        sampling._options(options)

@karva.tags.benchmark(rounds=1, iterations=1, warmup=0)
def test_single_use(benchmark, monkeypatch):
    monkeypatch.setattr(sampling.time, "perf_counter_ns", lambda: 0)
    assert benchmark(int, "42") == 42
    with karva.raises(RuntimeError, match="only measure one callable"):
        benchmark(int, "42")

def test_async_rejected(benchmark):
    async def operation():
        return 42
    with karva.raises(TypeError, match="synchronous callable"):
        benchmark(operation)
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @r#"
    success: true
    exit_code: 0
    ----- stdout -----

    benchmarks (nanoseconds per call):
      test_contract::test_single_use(benchmark, monkeypatch) [group: ungrouped]
        min=0ns median=0ns mean=0ns stddev=0ns
        warmup=0 calibration=0 rounds=1 iterations=1 measured=1 total=1 elapsed=0ns stop=rounds
        options: {"disable_gc":false,"group":null,"iterations":1,"max_time":0.1,"min_rounds":5,"min_time":0.001,"rounds":1,"warmup":0}

    ────────────
         Summary [TIME] 10 tests run: 10 passed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn inherits_groups_and_serializes_jsonl_through_wrapper_fixtures() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r"
import karva

@karva.fixture
def measure(benchmark):
    return benchmark
",
        ),
        (
            "test_group.py",
            r#"
import karva
import pytest
import karva._benchmark as sampling

pytestmark = pytest.mark.benchmark(group="module", iterations=1, rounds=1, warmup=0)

def test_inherited(measure, monkeypatch):
    monkeypatch.setattr(sampling.time, "perf_counter_ns", lambda: 0)
    assert measure(int, "42") == 42

@karva.tags.benchmark(group="local")
def test_local(measure, monkeypatch):
    monkeypatch.setattr(sampling.time, "perf_counter_ns", lambda: 0)
    assert measure(int, "42") == 42
"#,
        ),
    ]);
    assert_cmd_snapshot!(context.command().args(["--num-workers=2", "--result-output=results.jsonl", "--result-format=jsonl"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_group::test_inherited(measure=<Benchmark>, monkeypatch)
            PASS [TIME] test_group::test_local(measure=<Benchmark>, monkeypatch)

    benchmarks (nanoseconds per call):
      test_group::test_local(measure=<Benchmark>, monkeypatch) [group: local]
        min=0ns median=0ns mean=0ns stddev=0ns
        warmup=0 calibration=0 rounds=1 iterations=1 measured=1 total=1 elapsed=0ns stop=rounds
        options: {"disable_gc":false,"group":"local","iterations":1,"max_time":0.1,"min_rounds":5,"min_time":0.001,"rounds":1,"warmup":0}
      test_group::test_inherited(measure=<Benchmark>, monkeypatch) [group: module]
        min=0ns median=0ns mean=0ns stddev=0ns
        warmup=0 calibration=0 rounds=1 iterations=1 measured=1 total=1 elapsed=0ns stop=rounds
        options: {"disable_gc":false,"group":"module","iterations":1,"max_time":0.1,"min_rounds":5,"min_time":0.001,"rounds":1,"warmup":0}

    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    "#);
    let report = context.read_file("results.jsonl");
    let tests = report
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("JSONL record"))
        .filter(|line| line["type"] == "test")
        .collect::<Vec<_>>();
    assert_eq!(tests.len(), 2);
    assert_eq!(tests[0]["benchmarks"][0]["group"], "module");
    assert_eq!(tests[1]["benchmarks"][0]["group"], "local");
}

#[test]
fn callable_exceptions_fail_tests_and_keep_partial_measurements() {
    let context = TestContext::with_file(
        "test_failure.py",
        r#"
import karva
import karva._benchmark as sampling

@karva.tags.benchmark(rounds=3, iterations=1, warmup=0, disable_gc=True)
def test_failure(benchmark, monkeypatch):
    monkeypatch.setattr(sampling.time, "perf_counter_ns", lambda: 0)
    calls = []
    def operation():
        calls.append(1)
        if len(calls) == 2:
            raise ValueError("benchmark callable failed")
        return 42
    benchmark(operation)
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().args(["--result-output=results.json"]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_failure::test_failure(benchmark, monkeypatch)

    failures:

    test_failure::test_failure(benchmark, monkeypatch):

    error[test-failure]: Test `test_failure` failed
     --> test_failure.py:6:5
      |
    6 | def test_failure(benchmark, monkeypatch):
      |     ^^^^^^^^^^^^
    info: Test ran with arguments:
    info:   `benchmark`: `<Benchmark>`
    info:   `monkeypatch`: `<MockEnv object>`
    info: Test failed here
      --> test_failure.py:12:13
       |
    12 |             raise ValueError("benchmark callable failed")
       |             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: benchmark callable failed


    benchmarks (nanoseconds per call):
      test_failure::test_failure(benchmark, monkeypatch) [group: ungrouped]
        min=0ns median=0ns mean=0ns stddev=0ns
        warmup=0 calibration=0 rounds=1 iterations=1 measured=2 total=2 elapsed=0ns stop=exception
        options: {"disable_gc":true,"group":null,"iterations":1,"max_time":0.1,"min_rounds":5,"min_time":0.001,"rounds":3,"warmup":0}

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
    let report: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report");
    let benchmark = &report["tests"][0]["benchmarks"][0];
    assert_eq!(benchmark["rounds"], 1);
    assert_eq!(benchmark["measurement_calls"], 2);
    assert_eq!(benchmark["stop_reason"], "exception");
}

#[rstest]
fn callable_and_decorator_preserve_results_and_report_statistics(
    #[values("karva.tags", "pytest.mark")] marker: &str,
    #[values(false, true)] decorator: bool,
) {
    let call = if decorator {
        "@benchmark\n    def result():\n        return calculate(40, increment=2)\n    assert result == 42"
    } else {
        "assert benchmark(calculate, 40, increment=2) == 42"
    };
    let context = TestContext::with_file(
        "test_bench.py",
        format!(
            r#"
import karva
import pytest
import karva._benchmark as sampling

@{marker}.benchmark(group="parser", rounds=3, iterations=2, warmup=0)
def test_parse(benchmark, monkeypatch):
    ticks = iter([0, 0, 20, 20, 60, 60, 120, 200])
    monkeypatch.setattr(sampling.time, "perf_counter_ns", lambda: next(ticks))
    calls = []
    def calculate(value, *, increment):
        calls.append(value)
        return value + increment
    {call}
    assert len(calls) == 6
    assert benchmark.stats["samples_ns"] == [10, 20, 30]
"#
        ),
    );
    context.write_file("karva.toml", "[profile.default.test]\nstrict-tags = true\n");
    insta::allow_duplicates! {
        assert_cmd_snapshot!(context.command().args(["--num-workers=2", "--result-output=results.json"]), @r#"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test_bench::test_parse(benchmark, monkeypatch)

        benchmarks (nanoseconds per call):
          test_bench::test_parse(benchmark, monkeypatch) [group: parser]
            min=10ns median=20ns mean=20ns stddev=10ns
            warmup=0 calibration=0 rounds=3 iterations=2 measured=6 total=6 elapsed=200ns stop=rounds
            options: {"disable_gc":false,"group":"parser","iterations":2,"max_time":0.1,"min_rounds":5,"min_time":0.001,"rounds":3,"warmup":0}

        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        "#);
    }
    let report: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report");
    insta::allow_duplicates! {
        assert_snapshot!(serde_json::to_string_pretty(&report["tests"][0]["benchmarks"]).expect("serialize benchmark"), @r#"
        [
          {
            "calibration_calls": 0,
            "disabled": false,
            "elapsed_ns": 200,
            "group": "parser",
            "iterations": 2,
            "mean_ns": 20,
            "measurement_calls": 6,
            "median_ns": 20,
            "min_ns": 10,
            "options": {
              "disable_gc": false,
              "group": "parser",
              "iterations": 2,
              "max_time": 0.1,
              "min_rounds": 5,
              "min_time": 0.001,
              "rounds": 3,
              "warmup": 0
            },
            "rounds": 3,
            "samples_ns": [
              10,
              20,
              30
            ],
            "stddev_ns": 10,
            "stop_reason": "rounds",
            "total_calls": 6,
            "warmup_calls": 0
          }
        ]
        "#);
    }
}

#[rstest]
fn disabled_measurement_calls_once_and_allows_parallel_tests(
    #[values(false, true)] configuration: bool,
) {
    let context = TestContext::with_file(
        "test_bench.py",
        r#"
import karva

@karva.tags.benchmark(rounds=3, iterations=2, warmup=2)
def test_correctness(benchmark):
    calls = []
    assert benchmark(lambda: calls.append(1)) is None
    assert calls == [1]
    assert benchmark.stats["disabled"]
    assert benchmark.stats["samples_ns"] == []

def test_normal():
    assert True
"#,
    );
    if configuration {
        context.write_file(
            "karva.toml",
            "[profile.default.test]\nbenchmark-disable = true\n",
        );
    }
    let mut command = context.command();
    command.args([
        "--num-workers=2",
        "--result-output=results.json",
        "--status-level=none",
    ]);
    if !configuration {
        command.arg("--benchmark-disable");
    }
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"elapsed=\d+ns", "elapsed=[TIME]");
    let _guard = settings.bind_to_scope();
    {
        insta::allow_duplicates! { assert_cmd_snapshot!(command, @r#"
        success: true
        exit_code: 0
        ----- stdout -----

        benchmarks (nanoseconds per call):
          test_bench::test_correctness(benchmark) [group: ungrouped]
            warmup=0 calibration=0 rounds=0 iterations=1 measured=1 total=1 elapsed=[TIME] stop=disabled
            options: {"disable_gc":false,"group":null,"iterations":2,"max_time":0.1,"min_rounds":5,"min_time":0.001,"rounds":3,"warmup":2}

        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        "#); }
    }
    let report: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report");
    let benchmark = &report["tests"]
        .as_array()
        .expect("tests")
        .iter()
        .find(|test| {
            test["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("test_correctness"))
        })
        .expect("benchmark test")["benchmarks"][0];
    assert_eq!(benchmark["total_calls"], 1);
    assert_eq!(benchmark["warmup_calls"], 0);
    assert_eq!(benchmark["calibration_calls"], 0);
    assert_eq!(benchmark["rounds"], 0);
    assert_eq!(benchmark["stop_reason"], "disabled");
}

#[test]
fn adaptive_sampling_reports_work_and_excludes_fixture_lifecycle() {
    let context = TestContext::with_file(
        "test_bench.py",
        r#"
import karva
import karva._benchmark as sampling

@karva.fixture
def clock(monkeypatch):
    class Clock:
        value = 0
        def read(self):
            return self.value
    clock = Clock()
    clock.value += 1000000000  # fixture setup must not enter the samples
    monkeypatch.setattr(sampling.time, "perf_counter_ns", clock.read)
    yield clock
    clock.value += 1000000000  # teardown must not enter the samples

@karva.tags.benchmark(min_time=0.001, max_time=0.01, min_rounds=5, warmup=1)
def test_adaptive(benchmark, clock):
    calls = []
    def operation():
        clock.value += 500000
        calls.append(1)
        return len(calls)
    result = benchmark(operation)
    stats = benchmark.stats
    assert stats["warmup_calls"] == 1
    assert stats["calibration_calls"] == 3
    assert stats["iterations"] == 2
    assert stats["rounds"] == 8
    assert stats["measurement_calls"] == 16
    assert stats["total_calls"] == result == 20
    assert stats["elapsed_ns"] == 10000000
    assert stats["samples_ns"] == [500000] * 8
    assert stats["stddev_ns"] == 0
"#,
    );
    assert_cmd_snapshot!(context.command().args(["--num-workers=2", "--result-output=results.json"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_bench::test_adaptive(benchmark, clock=<test_bench.clock.<locals>....)

    benchmarks (nanoseconds per call):
      test_bench::test_adaptive(benchmark, clock=<test_bench.clock.<locals>....) [group: ungrouped]
        min=500000ns median=500000ns mean=500000ns stddev=0ns
        warmup=1 calibration=3 rounds=8 iterations=2 measured=16 total=20 elapsed=10000000ns stop=max_time
        options: {"disable_gc":false,"group":null,"iterations":null,"max_time":0.01,"min_rounds":5,"min_time":0.001,"rounds":null,"warmup":1}

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "#);
    let report: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report");
    assert_snapshot!(serde_json::to_string_pretty(&report["tests"][0]["benchmarks"]).expect("serialize benchmark"), @r#"
    [
      {
        "calibration_calls": 3,
        "disabled": false,
        "elapsed_ns": 10000000,
        "group": null,
        "iterations": 2,
        "mean_ns": 500000,
        "measurement_calls": 16,
        "median_ns": 500000,
        "min_ns": 500000,
        "options": {
          "disable_gc": false,
          "group": null,
          "iterations": null,
          "max_time": 0.01,
          "min_rounds": 5,
          "min_time": 0.001,
          "rounds": null,
          "warmup": 1
        },
        "rounds": 8,
        "samples_ns": [
          500000,
          500000,
          500000,
          500000,
          500000,
          500000,
          500000,
          500000
        ],
        "stddev_ns": 0,
        "stop_reason": "max_time",
        "total_calls": 20,
        "warmup_calls": 1
      }
    ]
    "#);
}

#[test]
fn invalid_sampling_controls_fail_before_running_test() {
    let context = TestContext::with_file(
        "test_bench.py",
        r#"
import karva

@karva.tags.benchmark(rounds=0)
def test_invalid(benchmark):
    raise AssertionError("test body should not run")
"#,
    );
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(
        r"[^ \n]*site-packages/karva/_benchmark.py",
        "<karva>/_benchmark.py",
    );
    let _guard = settings.bind_to_scope();
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_bench::test_invalid

    failures:

    test_bench::test_invalid:

    error[test-failure]: Test `test_invalid` failed
     --> test_bench.py:5:5
      |
    5 | def test_invalid(benchmark):
      |     ^^^^^^^^^^^^
    info: Test failed here
      --> <karva>/_benchmark.py:55:13
       |
    55 |             raise ValueError(
       |             ^^^^^^^^^^^^^^^^^
    info: benchmark rounds must be an integer >= 1; requested 0

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn failures_preserve_partial_samples_and_restore_gc_across_retries() {
    let context = TestContext::with_file(
        "test_bench.py",
        r#"
import gc
import os
import karva
import karva._benchmark as sampling

@karva.tags.benchmark(rounds=3, iterations=1, warmup=0, disable_gc=True)
def test_retry(benchmark, monkeypatch):
    ticks = iter(range(0, 1000, 10))
    monkeypatch.setattr(sampling.time, "perf_counter_ns", lambda: next(ticks))
    calls = []
    def operation():
        assert not gc.isenabled()
        calls.append(1)
        if len(calls) == 2 and os.environ["KARVA_ATTEMPT"] == "1":
            raise ValueError("measurement failed")
        return 42
    try:
        assert benchmark(operation) == 42
    finally:
        assert gc.isenabled()
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().args(["--retry=1", "--result-output=results.json"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test_bench::test_retry(benchmark, monkeypatch)
      TRY 2 PASS [TIME] test_bench::test_retry(benchmark, monkeypatch)

    benchmarks (nanoseconds per call):
      test_bench::test_retry(benchmark, monkeypatch) [group: ungrouped]
        min=10ns median=10ns mean=10ns stddev=0ns
        warmup=0 calibration=0 rounds=3 iterations=1 measured=3 total=3 elapsed=70ns stop=rounds
        options: {"disable_gc":true,"group":null,"iterations":1,"max_time":0.1,"min_rounds":5,"min_time":0.001,"rounds":3,"warmup":0}

    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test_bench::test_retry(benchmark, monkeypatch)

    ----- stderr -----
    "#);
    let report: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report");
    assert_snapshot!(serde_json::to_string_pretty(&report["tests"][0]["attempts"][0]["benchmarks"]).expect("serialize benchmark"), @r#"
    [
      {
        "calibration_calls": 0,
        "disabled": false,
        "elapsed_ns": 40,
        "group": null,
        "iterations": 1,
        "mean_ns": 10,
        "measurement_calls": 2,
        "median_ns": 10,
        "min_ns": 10,
        "options": {
          "disable_gc": true,
          "group": null,
          "iterations": 1,
          "max_time": 0.1,
          "min_rounds": 5,
          "min_time": 0.001,
          "rounds": 3,
          "warmup": 0
        },
        "rounds": 1,
        "samples_ns": [
          10
        ],
        "stddev_ns": 0,
        "stop_reason": "exception",
        "total_calls": 2,
        "warmup_calls": 0
      }
    ]
    "#);
}

#[test]
fn hidden_benchmarks_reject_parallel_measurement() {
    let context = TestContext::with_files([
        (
            "helpers.py",
            r"
import karva

@karva.fixture
def measure(benchmark):
    return benchmark
",
        ),
        (
            "test_hidden.py",
            r#"
from helpers import measure

def test_hidden(measure):
    measure(int, "42")

def test_normal():
    pass
"#,
        ),
    ]);
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(
        r"[^ \n]*site-packages/karva/_benchmark.py",
        "<karva>/_benchmark.py",
    );
    let _guard = settings.bind_to_scope();
    assert_cmd_snapshot!(context.command().args(["--num-workers=2", "--try-import-fixtures", "--status-level=fail"]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 2 workers
            FAIL [TIME] test_hidden::test_hidden(measure=<Benchmark>)

    failures:

    test_hidden::test_hidden(measure=<Benchmark>):

    error[test-failure]: Test `test_hidden` failed
     --> test_hidden.py:4:5
      |
    4 | def test_hidden(measure):
      |     ^^^^^^^^^^^
    info: Test ran with arguments:
    info:   `measure`: `<Benchmark>`
    info: Test failed here
     --> test_hidden.py:5:5
      |
    5 |     measure(int, "42")
      |     ^^^^^^^^^^^^^^^^^^
    info: benchmark worker budget exceeded: requested 2, limit 1; run with --num-workers=1

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn retains_successful_and_partial_teardown_measurements() {
    let context = TestContext::with_file(
        "test_teardown.py",
        r#"
import karva
import karva._benchmark as sampling

@karva.fixture
def measured(benchmark, monkeypatch):
    class Clock:
        value = 0
        def read(self):
            return self.value
    clock = Clock()
    monkeypatch.setattr(sampling.time, "perf_counter_ns", clock.read)
    state = {}
    yield state
    clock.value += 1000000000
    calls = []
    def operation():
        clock.value += 10
        calls.append(1)
        if state["fail"] and len(calls) == 2:
            raise ValueError("teardown benchmark failed")
        return 42
    try:
        assert benchmark(operation) == 42
    finally:
        clock.value += 1000000000

@karva.tags.benchmark(group="teardown", rounds=3, iterations=1, warmup=0)
@karva.tags.parametrize("fail", [False, True])
def test_teardown(measured, fail):
    measured["fail"] = fail
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().arg("--result-output=results.json"), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_teardown::test_teardown(measured={}, fail=False)
           ERROR [TIME] test_teardown::test_teardown(measured={}, fail=True)

    failures:

    test_teardown::test_teardown(measured={}, fail=True):

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `measured`
     --> test_teardown.py:6:5
      |
    6 | def measured(benchmark, monkeypatch):
      |     ^^^^^^^^
    info: Failed to reset fixture: teardown benchmark failed


    benchmarks (nanoseconds per call):
      test_teardown::test_teardown(measured={}, fail=False) [group: teardown]
        min=10ns median=10ns mean=10ns stddev=0ns
        warmup=0 calibration=0 rounds=3 iterations=1 measured=3 total=3 elapsed=30ns stop=rounds
        options: {"disable_gc":false,"group":"teardown","iterations":1,"max_time":0.1,"min_rounds":5,"min_time":0.001,"rounds":3,"warmup":0}
      test_teardown::test_teardown(measured={}, fail=True) [group: teardown]
        min=10ns median=10ns mean=10ns stddev=0ns
        warmup=0 calibration=0 rounds=1 iterations=1 measured=2 total=2 elapsed=20ns stop=exception
        options: {"disable_gc":false,"group":"teardown","iterations":1,"max_time":0.1,"min_rounds":5,"min_time":0.001,"rounds":3,"warmup":0}

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 error, 0 skipped

    ----- stderr -----
    "#);
    let report: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report");
    let passed = &report["tests"][0]["benchmarks"][0];
    assert_eq!(passed["rounds"], 3);
    assert_eq!(passed["measurement_calls"], 3);
    assert_eq!(passed["samples_ns"], serde_json::json!([10, 10, 10]));
    assert_eq!(passed["elapsed_ns"], 30);
    assert_eq!(passed["stop_reason"], "rounds");
    let failed = &report["tests"][1]["benchmarks"][0];
    assert_eq!(failed["rounds"], 1);
    assert_eq!(failed["measurement_calls"], 2);
    assert_eq!(failed["samples_ns"], serde_json::json!([10]));
    assert_eq!(failed["elapsed_ns"], 20);
    assert_eq!(failed["stop_reason"], "exception");
}
