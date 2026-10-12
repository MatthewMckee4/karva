//! Warning capture, policy, isolation, and report behavior across worker boundaries.

use std::fmt::Write as _;

use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;
use serde_json::Value;

use crate::common::TestContext;

#[test]
fn captures_unconsumed_warnings_in_every_phase_and_report() {
    let context = TestContext::new();
    context.write_file(
        "test.py",
        r#"
import warnings
import karva

warnings.warn("import warning", DeprecationWarning)

@karva.fixture
def value():
    warnings.warn("setup warning", ResourceWarning)
    yield 1
    warnings.warn("teardown warning", RuntimeWarning)

@karva.fixture(scope="session", auto_use=True)
def session():
    warnings.warn("session setup")
    yield
    warnings.warn("session teardown")

def test_warn(value):
    warnings.warn("call warning", DeprecationWarning)
    assert value == 1

def test_consumed(recwarn):
    warnings.warn("consumed fixture")
    assert len(recwarn) == 1

def test_recorded():
    with karva.warns(UserWarning):
        warnings.warn("consumed helper")
    with warnings.catch_warnings(record=True) as recorded:
        warnings.warn("consumed context")
        assert len(recorded) == 1
"#,
    );
    context.write_file(
        "karva.toml",
        "[profile.default.junit]\npath = \"results.xml\"\n",
    );
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--status-level=none", "--result-output=results.json",]), @"
    success: true
    exit_code: 0
    ----- stdout -----

    warnings:
      test.py:5: DeprecationWarning: import warning (1 emitted)
      test.py:9: ResourceWarning: setup warning (1 emitted)
      test.py:11: RuntimeWarning: teardown warning (1 emitted)
      test.py:15: UserWarning: session setup (1 emitted)
      test.py:17: UserWarning: session teardown (1 emitted)
      test.py:20: DeprecationWarning: call warning (1 emitted)

    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
    let report: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report");
    let warnings = report["warnings"].as_array().expect("warnings");
    assert_eq!(warnings.len(), 6);
    assert_eq!(
        warnings
            .iter()
            .filter(|warning| warning["test"].is_string())
            .count(),
        3
    );
    assert!(warnings.iter().all(|warning| {
        !warning["message"]
            .as_str()
            .expect("message")
            .contains("consumed")
    }));
    let xml = context.read_file("results.xml");
    let duration = regex::Regex::new(r#"time="[0-9.]+""#).expect("duration regex");
    assert_snapshot!(duration.replace_all(&xml, "time=\"[TIME]\""), @r#"
    <?xml version="1.0" encoding="UTF-8"?>
    <testsuites name="karva-tests" tests="3" failures="0" skipped="0" errors="0" time="[TIME]">
      <testsuite name="test" tests="3" failures="0" skipped="0" errors="0" time="[TIME]">
        <testcase classname="test" name="test_consumed(recwarn)" time="[TIME]"/>
        <testcase classname="test" name="test_recorded" time="[TIME]"/>
        <testcase classname="test" name="test_warn(value=1)" time="[TIME]">
          <system-err>test.py:20: DeprecationWarning: call warning (attempt 1)
    test.py:9: ResourceWarning: setup warning (attempt 1)
    test.py:11: RuntimeWarning: teardown warning (attempt 1)
    </system-err>
        </testcase>
      </testsuite>
      <testsuite name="warnings" tests="0" failures="0" skipped="0" errors="0" time="[TIME]">
          <system-err>test.py:5: DeprecationWarning: import warning
    test.py:15: UserWarning: session setup
    test.py:17: UserWarning: session teardown
    </system-err>
      </testsuite>
    </testsuites>
    "#);
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--status-level=none",
        "--result-output=results.jsonl",
        "--result-format=jsonl",
    ]), @"
    success: true
    exit_code: 0
    ----- stdout -----

    warnings:
      test.py:5: DeprecationWarning: import warning (1 emitted)
      test.py:9: ResourceWarning: setup warning (1 emitted)
      test.py:11: RuntimeWarning: teardown warning (1 emitted)
      test.py:15: UserWarning: session setup (1 emitted)
      test.py:17: UserWarning: session teardown (1 emitted)
      test.py:20: DeprecationWarning: call warning (1 emitted)

    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
    let jsonl = context.read_file("results.jsonl");
    let warnings = jsonl
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("JSONL record"))
        .filter(|record| record["type"] == "warning")
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), 6);
    assert!(
        warnings
            .iter()
            .any(|warning| warning["test"] == "test::test_warn(value=1)" && warning["attempt"] == 1)
    );
}

#[rstest]
fn repeated_warnings_are_counted_across_workers(#[values(1, 2)] workers: usize) {
    let context = TestContext::new();
    let mut source =
        "import warnings\n\ndef emit():\n    warnings.warn('repeated', DeprecationWarning)\n\n"
            .to_string();
    for index in 0..10 {
        write!(source, "def test_{index}():\n    emit()\n    emit()\n\n")
            .expect("generate test source");
    }
    context.write_file("test.py", &source);
    insta::allow_duplicates! {
        assert_cmd_snapshot!(context.command().args([
            &format!("--num-workers={workers}"), "--status-level=none", "--max-warnings=9",
            "--result-output=results.json",
        ]), @"
        success: false
        exit_code: 1
        ----- stdout -----

        diagnostics:

        error[max-warnings]: max-warnings budget exceeded: limit 9, emitted 10

        warnings:
          test.py:4: DeprecationWarning: repeated (10 emitted)

        ────────────
             Summary [TIME] 10 tests run: 10 passed, 0 skipped

        ----- stderr -----
        ");
    }
    let report: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report");
    assert_eq!(report["warnings"].as_array().expect("warnings").len(), 10);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["stats"]["passed"], 10);
    insta::allow_duplicates! {
        assert_cmd_snapshot!(context.command().args([
            &format!("--num-workers={workers}"), "--status-level=none", "--max-warnings=10",
        ]), @"
        success: true
        exit_code: 0
        ----- stdout -----

        warnings:
          test.py:4: DeprecationWarning: repeated (10 emitted)

        ────────────
             Summary [TIME] 10 tests run: 10 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn profiles_cli_and_local_tags_have_ordered_precedence() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import warnings
import karva
import pytest

@karva.tags.filter_warnings("ignore:local:DeprecationWarning")
def test_karva():
    warnings.warn("local", DeprecationWarning)

@pytest.mark.filterwarnings("ignore:local:DeprecationWarning")
def test_pytest():
    warnings.warn("local", DeprecationWarning)

def test_visible():
    warnings.warn("visible", DeprecationWarning)
"#,
    );
    context.write_file(
        "karva.toml",
        r#"
[profile.default.test]
warning-filters = ["ignore"]
[profile.ci.test]
warning-filters = ["error", "ignore:visible:DeprecationWarning"]
"#,
    );
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--profile=ci", "--status-level=none"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--profile=ci",
        "--status-level=none",
        "-W",
        "default",
    ]), @"
    success: true
    exit_code: 0
    ----- stdout -----

    warnings:
      test.py:15: DeprecationWarning: visible (1 emitted)

    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--status-level=none",
        "-W",
        "error",
    ]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test::test_visible:

    error[test-failure]: Test `test_visible` failed
      --> test.py:14:5
       |
    14 | def test_visible():
       |     ^^^^^^^^^^^^
    info: Test failed here
      --> test.py:15:5
       |
    15 |     warnings.warn("visible", DeprecationWarning)
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: visible

    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn isolates_filters_registries_parameters_and_retries() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import warnings
import os
import karva

@karva.tags.parametrize("value", [1, 2])
def test_case(value):
    warnings.warn("case", DeprecationWarning)
    warnings.simplefilter("ignore")
    assert int(os.environ["KARVA_ATTEMPT"]) == 2
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--retry=1",
        "--status-level=none",
        "--result-output=results.json",
    ]), @"
    success: true
    exit_code: 0
    ----- stdout -----

    warnings:
      test.py:8: DeprecationWarning: case (4 emitted)

    ────────────
         Summary [TIME] 2 tests run: 2 passed (2 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_case(value=1)
       FLAKY 2/2 [TIME] test::test_case(value=2)

    ----- stderr -----
    ");
    let report: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report");
    let warnings = report["warnings"].as_array().expect("warnings");
    assert_eq!(warnings.len(), 4);
    assert_eq!(
        warnings
            .iter()
            .filter(|warning| warning["attempt"] == 1)
            .count(),
        2
    );
    assert_eq!(
        warnings
            .iter()
            .filter(|warning| warning["attempt"] == 2)
            .count(),
        2
    );
}

#[rstest]
#[case("bad-action")]
#[case("ignore::UnknownWarning")]
#[case("ignore::ValueError")]
#[case("ignore:[")]
#[case("ignore::::-1")]
#[case("ignore::::x")]
#[case("ignore::::1:extra")]
fn rejects_invalid_project_filters(#[case] filter: &str) {
    let context = TestContext::with_file("test.py", "def test_pass():\n    assert False\n");
    context.write_file(
        "karva.toml",
        &format!("[profile.default.test]\nwarning-filters = [{filter:?}]\n"),
    );
    let mut command = context.command_no_parallel();
    command.arg("--status-level=none");
    match filter {
        "bad-action" => assert_cmd_snapshot!(command, @"
        success: false
        exit_code: 1
        ----- stdout -----
        diagnostics:

        error[warning-policy]: ValueError: invalid warning filter 'bad-action': unknown action 'bad-action'

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        "),
        "ignore::UnknownWarning" => assert_cmd_snapshot!(command, @"
        success: false
        exit_code: 1
        ----- stdout -----
        diagnostics:

        error[warning-policy]: ValueError: invalid warning filter 'ignore::UnknownWarning': unknown category 'UnknownWarning'

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        "),
        "ignore::ValueError" => assert_cmd_snapshot!(command, @"
        success: false
        exit_code: 1
        ----- stdout -----
        diagnostics:

        error[warning-policy]: ValueError: invalid warning filter 'ignore::ValueError': 'ValueError' is not a warning category

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        "),
        "ignore:[" => assert_cmd_snapshot!(command, @"
        success: false
        exit_code: 1
        ----- stdout -----
        diagnostics:

        error[warning-policy]: ValueError: invalid warning filter 'ignore:[': unterminated character set at position 0

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        "),
        "ignore::::-1" => assert_cmd_snapshot!(command, @"
        success: false
        exit_code: 1
        ----- stdout -----
        diagnostics:

        error[warning-policy]: ValueError: invalid warning filter 'ignore::::-1': line must be non-negative

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        "),
        "ignore::::x" => assert_cmd_snapshot!(command, @"
        success: false
        exit_code: 1
        ----- stdout -----
        diagnostics:

        error[warning-policy]: ValueError: invalid warning filter 'ignore::::x': invalid literal for int() with base 10: 'x'

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        "),
        _ => {
            assert_eq!(filter, "ignore::::1:extra");
            assert_cmd_snapshot!(command, @"
            success: false
            exit_code: 1
            ----- stdout -----
            diagnostics:

            error[warning-policy]: ValueError: invalid warning filter 'ignore::::1:extra': too many fields

            ────────────
                 Summary [TIME] 0 tests run: 0 passed, 0 skipped

            ----- stderr -----
            ");
        }
    }
}

#[rstest]
fn rejects_invalid_local_filters(
    #[values("karva.tags.filter_warnings", "pytest.mark.filterwarnings")] decorator: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
import karva
import pytest
@{decorator}("ignore::UnknownWarning")
def test_never_runs():
    assert False
"#
        ),
    );
    insta::allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
        success: false
        exit_code: 1
        ----- stdout -----
        diagnostics:

        error[failed-to-import-module]: Failed to import python module `test`: invalid warning filter 'ignore::UnknownWarning': unknown category 'UnknownWarning'

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn restores_registry_objects_and_new_module_registries() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import sys
import warnings
import karva

original = {"sentinel": 1}
__warningregistry__ = original

@karva.fixture(scope="session", auto_use=True)
def verify_restoration():
    filters = warnings.filters[:]
    yield
    assert __warningregistry__ is original
    assert __warningregistry__ == {"sentinel": 1}
    assert warnings.filters == filters
    assert not hasattr(sys.modules["imported"], "__warningregistry__")

@karva.tags.parametrize("value", [1, 2])
def test_modify(value):
    import imported
    assert __warningregistry__ == {}
    warnings.warn("registry warning")
    warnings.filters.clear()
"#,
    );
    context.write_file(
        "imported.py",
        "import warnings\nwarnings.warn('imported during test')\n",
    );
    assert_cmd_snapshot!(context.command_no_parallel().args(["--status-level=none"]), @"
    success: true
    exit_code: 0
    ----- stdout -----

    warnings:
      imported.py:2: UserWarning: imported during test (1 emitted)
      test.py:22: UserWarning: registry warning (2 emitted)

    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn dotted_categories_message_module_and_line_filters() {
    let context = TestContext::with_file(
        "categories.py",
        "class CustomWarning(ResourceWarning):\n    pass\n",
    );
    context.write_file("test.py", r#"
import warnings
import karva
import categories

@karva.tags.filter_warnings("error", "a:allowed:categories.CustomWarning:^target.module$:42")
def test_filters():
    for _ in range(2):
        warnings.warn_explicit("allowed", categories.CustomWarning, "custom.py", 42, module="target.module")
    with karva.raises(categories.CustomWarning):
        warnings.warn_explicit("allowed", categories.CustomWarning, "custom.py", 43, module="target.module")
    with karva.raises(categories.CustomWarning):
        warnings.warn_explicit("allowed", categories.CustomWarning, "custom.py", 42, module="other.module")
    with karva.raises(categories.CustomWarning):
        warnings.warn_explicit("different", categories.CustomWarning, "custom.py", 42, module="target.module")
"#);
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--status-level=none", "--max-warnings=1"]), @"
    success: false
    exit_code: 1
    ----- stdout -----

    diagnostics:

    error[max-warnings]: max-warnings budget exceeded: limit 1, emitted 2

    warnings:
      custom.py:42: CustomWarning: allowed (2 emitted)

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn strict_policy_applies_to_import_setup_and_teardown(
    #[values("import", "setup", "teardown")] phase: &str,
) {
    let body = match phase {
        "import" => "warnings.warn('strict', ResourceWarning)\ndef test_strict():\n    pass\n",
        "setup" => {
            "@karva.fixture\ndef value():\n    warnings.warn('strict', ResourceWarning)\n    return 1\ndef test_strict(value):\n    pass\n"
        }
        _ => {
            "@karva.fixture\ndef value():\n    yield 1\n    warnings.warn('strict', ResourceWarning)\ndef test_strict(value):\n    pass\n"
        }
    };
    let context =
        TestContext::with_file("test.py", &format!("import warnings\nimport karva\n{body}"));
    let mut command = context.command_no_parallel();
    command.args(["--status-level=none", "-W", "error"]);
    match phase {
        "import" => assert_cmd_snapshot!(command, @"
        success: false
        exit_code: 1
        ----- stdout -----
        diagnostics:

        error[failed-to-import-module]: Failed to import python module `test`: strict

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        "),
        "setup" => assert_cmd_snapshot!(command, @"
        success: false
        exit_code: 1
        ----- stdout -----

        failures:

        test::test_strict (requires fixture `value`):

        error[fixture-failure]: Fixture `value` failed
         --> test.py:4:5
          |
        4 | def value():
          |     ^^^^^
        info: Fixture failed here
         --> test.py:5:5
          |
        5 |     warnings.warn('strict', ResourceWarning)
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: strict

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "),
        _ => {
            assert_eq!(phase, "teardown");
            assert_cmd_snapshot!(command, @"
            success: false
            exit_code: 1
            ----- stdout -----

            failures:

            test::test_strict(value=1):

            error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `value`
             --> test.py:4:5
              |
            4 | def value():
              |     ^^^^^
            info: Failed to reset fixture: strict

            ────────────
                 Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

            ----- stderr -----
            ");
        }
    }
}

#[test]
fn pytest_module_and_parameter_filters_are_local() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import warnings
import pytest

pytestmark = pytest.mark.filterwarnings("ignore:module warning")

@pytest.mark.parametrize("value", [pytest.param(1, marks=pytest.mark.filterwarnings("ignore:parameter warning")), 2])
def test_parameters(value):
    warnings.warn("module warning")
    warnings.warn("parameter warning")
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--status-level=none",
        "-W",
        "error"
    ]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test::test_parameters(value=2):

    error[test-failure]: Test `test_parameters` failed
     --> test.py:8:5
      |
    8 | def test_parameters(value):
      |     ^^^^^^^^^^^^^^^
    info: Test ran with arguments:
    info:   `value`: `2`
    info: Test failed here
      --> test.py:10:5
       |
    10 |     warnings.warn("parameter warning")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: parameter warning

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[rstest]
fn function_filters_override_module_filters_and_outer_decorators_win(
    #[values(
        "pytestmark = pytest.mark.filterwarnings(\"error\")",
        "karva_tag = karva.tags.filter_warnings(\"error\")"
    )]
    module_filter: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
import warnings
import pytest
import karva

{module_filter}

@pytest.mark.filterwarnings("ignore:local")
@pytest.mark.filterwarnings("error")
def test_pytest():
    warnings.warn("local")

@karva.tags.filter_warnings("ignore:local")
@karva.tags.filter_warnings("error")
def test_karva():
    warnings.warn("local")
"#
        ),
    );
    insta::allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel().args(["--status-level=none"]), @"
        success: true
        exit_code: 0
        ----- stdout -----
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn cli_overrides_profile_warning_budget() {
    let context = TestContext::with_file(
        "test.py",
        "import warnings\ndef test_warn():\n    warnings.warn('budget warning')\n",
    );
    context.write_file("karva.toml", "[profile.default.test]\nmax-warnings = 0\n");
    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: false
    exit_code: 1
    ----- stdout -----

    diagnostics:

    error[max-warnings]: max-warnings budget exceeded: limit 0, emitted 1

    warnings:
      test.py:3: UserWarning: budget warning (1 emitted)

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--status-level=none", "--max-warnings=1"]), @"
    success: true
    exit_code: 0
    ----- stdout -----

    warnings:
      test.py:3: UserWarning: budget warning (1 emitted)

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}
