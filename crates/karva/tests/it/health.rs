use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn health_does_not_execute_python() {
    let context = TestContext::with_file(
        "test_health.py",
        "raise RuntimeError('must not import')\ndef test_ok(tmp_path): pass\n",
    );
    assert_cmd_snapshot!(context.karva_command_in(context.root()).arg("health"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    0 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
}

#[test]
fn health_reports_and_suppresses_a_fixture_cycle() {
    let context = TestContext::with_file(
        "test_health.py",
        "import karva\n@karva.fixture\ndef cycle(cycle): pass\ndef test_cycle(cycle): pass\n",
    );
    assert_cmd_snapshot!(context.karva_command_in(context.root()).args(["health", "--format=json"]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    {
      "schema_version": 1,
      "analysis": "source-only",
      "findings": [
        {
          "rule": "fixture-cycle",
          "severity": "error",
          "location": {
            "path": "test_health.py",
            "line": 3,
            "column": 11,
            "start": 38,
            "end": 43
          },
          "evidence": "Fixture dependency cycle: cycle -> cycle",
          "related": [
            {
              "location": {
                "path": "test_health.py",
                "line": 3,
                "column": 5,
                "start": 32,
                "end": 37
              },
              "evidence": "Fixture `cycle` participates in this cycle"
            }
          ],
          "action": "Remove a dependency from the fixture cycle."
        }
      ]
    }

    ----- stderr -----
    "#);
    assert_cmd_snapshot!(context.karva_command_in(context.root()).args(["health", "--ignore=fixture-cycle"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    0 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
    assert_cmd_snapshot!(context.karva_command_in(context.root()).args(["health", "--fail-on=none"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    test_health.py:3:11: error[fixture-cycle]: Fixture dependency cycle: cycle -> cycle
      test_health.py:3:5: Fixture `cycle` participates in this cycle
      help: Remove a dependency from the fixture cycle.
    1 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
}

#[test]
fn health_honors_paths_profile_and_ancestor_fixtures() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            "[profile.ci.src]\ninclude = ['tests/chosen.py']\n",
        ),
        (
            "conftest.py",
            "import karva\n@karva.fixture\ndef database(): pass\n",
        ),
        ("tests/chosen.py", "def test_ok(database): pass\n"),
        ("tests/other.py", "def test_bad(missing): pass\n"),
    ]);
    assert_cmd_snapshot!(context.karva_command_in(context.root()).args(["health", "--profile=ci"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    0 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
    assert_cmd_snapshot!(context.karva_command_in(context.root()).args(["health", "tests/chosen.py::test_ok"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    0 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
}

#[test]
fn health_shadowed_definitions() {
    let context = TestContext::with_file(
        "test_health.py",
        r"
import karva
from karva import fixture as provider

def test_lost(): pass
def test_lost(): pass
def test_lost(): pass

class Tests:
    def test_method(self): pass
    def test_method(self): pass

@provider
def data(): pass
@provider
def data(): pass

if unknown:
    def test_conditional(): pass
if other:
    def test_conditional(): pass
",
    );
    assert_cmd_snapshot!(context.karva_command_in(context.root()).arg("health"), @"
    success: false
    exit_code: 1
    ----- stdout -----
    test_health.py:5:5: error[shadowed-definition]: Later definition `test_lost` replaces the earlier test or fixture
      test_health.py:7:5: Surviving definition
      help: Rename or remove one definition so each test or fixture has a distinct name.
    test_health.py:6:5: error[shadowed-definition]: Later definition `test_lost` replaces the earlier test or fixture
      test_health.py:7:5: Surviving definition
      help: Rename or remove one definition so each test or fixture has a distinct name.
    test_health.py:10:9: error[shadowed-definition]: Later definition `test_method` replaces the earlier test or fixture
      test_health.py:11:9: Surviving definition
      help: Rename or remove one definition so each test or fixture has a distinct name.
    test_health.py:14:5: error[shadowed-definition]: Later definition `data` replaces the earlier test or fixture
      test_health.py:16:5: Surviving definition
      help: Rename or remove one definition so each test or fixture has a distinct name.
    test_health.py:16:5: error[duplicate-fixture]: Fixture `data` is defined more than once
      test_health.py:14:5: First definition is here
      help: Rename or remove the duplicate fixture.
    test_health.py:19:9: warning[possibly-shadowed-definition]: Definition `test_conditional` may replace the earlier definition if both statements execute
      test_health.py:21:9: Potential surviving definition
      help: Rename or remove one definition so each test or fixture has a distinct name.
    6 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
}

#[test]
fn health_exclusive_definitions_and_distinct_scopes_are_clean() {
    let context = TestContext::with_file(
        "test_health.py",
        r"
if condition:
    def test_choice(): pass
else:
    def test_choice(): pass
if False:
    def test_static(): pass
def test_static(): pass
match value:
    case 1:
        def test_match(): pass
    case _:
        def test_match(): pass
class First:
    def test_method(self): pass
class Second:
    def test_method(self): pass
",
    );
    assert_cmd_snapshot!(context.karva_command_in(context.root()).arg("health"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    0 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
}

#[test]
fn health_uncertain_shadowing_has_warning_exit_policy() {
    let context = TestContext::with_file(
        "test_health.py",
        "def test_case(): pass\nif unknown:\n    def test_case(): pass\n",
    );
    assert_cmd_snapshot!(context.karva_command_in(context.root()).arg("health"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    test_health.py:1:5: warning[possibly-shadowed-definition]: Definition `test_case` may replace the earlier definition if both statements execute
      test_health.py:3:9: Potential surviving definition
      help: Rename or remove one definition so each test or fixture has a distinct name.
    1 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
    assert_cmd_snapshot!(context.karva_command_in(context.root()).args(["health", "--fail-on=warning"]), @"
    success: false
    exit_code: 1
    ----- stdout -----
    test_health.py:1:5: warning[possibly-shadowed-definition]: Definition `test_case` may replace the earlier definition if both statements execute
      test_health.py:3:9: Potential surviving definition
      help: Rename or remove one definition so each test or fixture has a distinct name.
    1 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
}

#[test]
fn health_duplicate_literal_cases_and_explicit_ids() {
    let context = TestContext::with_file(
        "test_health.py",
        r#"
import karva
from karva import param as case
from karva.tags import parametrize as cases

@cases("value", [{"a": [1, (2, None)]}, {'a': [0x1, (2, None)]}])
def test_nested(value): pass

@cases("value", [1, 2], ids=["same", "same"])
def test_ids(value): pass

@cases("value", [case(3, id="x"), case(4, id="x")])
def test_explicit_ids(value): pass
"#,
    );
    assert_cmd_snapshot!(context.karva_command_in(context.root()).args(["health", "--fail-on=warning"]), @"
    success: false
    exit_code: 1
    ----- stdout -----
    test_health.py:6:41: warning[duplicate-parameter-case]: Parameter case 2 repeats literal arguments from case 1
      test_health.py:6:18: First occurrence: case 1
      help: Remove the repeated literal case, or suppress this rule if repetition is intentional.
    test_health.py:9:38: warning[duplicate-parameter-id]: Parameter case 2 repeats the explicit ID from case 1
      test_health.py:9:30: First occurrence: case 1
      help: Give each explicit case ID a distinct name.
    test_health.py:12:46: warning[duplicate-parameter-id]: Parameter case 2 repeats the explicit ID from case 1
      test_health.py:12:29: First occurrence: case 1
      help: Give each explicit case ID a distinct name.
    3 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
}

#[test]
fn health_dynamic_and_distinct_parameters_are_clean() {
    let context = TestContext::with_file(
        "test_health.py",
        r#"
import karva
@karva.tags.parametrize("value", [dynamic(), dynamic(), [1], (1,), 1, True])
def test_unknown(value): pass
@karva.tags.parametrize("value", [*unknown, 1, 1])
def test_unpacked_values(value): pass
@karva.tags.parametrize("value", [1, 2], ids=[*unknown_ids, "same", "same"])
def test_unpacked_ids(value): pass
@karva.tags.parametrize("value", [1, 2], ids=make_ids)
def test_dynamic_ids(value): pass
@karva.tags.parametrize("value", [karva.param(1, id=dynamic()), karva.param(2, id=dynamic())], ids=["same", "same"])
def test_dynamic_explicit_ids(value): pass
@karva.tags.parametrize("value", [dynamic(), dynamic()], ids=["same", "same"])
def test_dynamic_cases_with_ids(value): pass
@karva.tags.parametrize("value", [karva.param(1), karva.param(1, tags=[karva.tags.skip])])
def test_different_marks(value): pass
"#,
    );
    assert_cmd_snapshot!(context.karva_command_in(context.root()).arg("health"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    0 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
}

#[test]
fn health_unused_fixture_graph() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            "import karva\n@karva.fixture\ndef inherited(): pass\n@karva.fixture\ndef abandoned(): pass\n",
        ),
        (
            "test_health.py",
            r"
import karva
@karva.fixture
def dependency(): pass
@karva.fixture
def used(dependency): pass
@karva.fixture(auto_use=True)
def automatic(): pass
@karva.fixture
def unused_dependency(): pass
@karva.fixture
def unused(unused_dependency): pass
@karva.fixture
def inherited(): pass
def test_ok(used, inherited): pass
",
        ),
    ]);
    assert_cmd_snapshot!(context.karva_command_in(context.root()).args(["health", "--fail-on=warning"]), @"
    success: false
    exit_code: 1
    ----- stdout -----
    conftest.py:3:5: warning[unused-fixture]: Fixture `inherited` has no direct or transitive consumer in the full configured suite
      help: Remove the fixture, or explicitly export it if it is a public fixture API.
    conftest.py:5:5: warning[unused-fixture]: Fixture `abandoned` has no direct or transitive consumer in the full configured suite
      help: Remove the fixture, or explicitly export it if it is a public fixture API.
    test_health.py:10:5: warning[unused-fixture]: Fixture `unused_dependency` has no direct or transitive consumer in the full configured suite
      help: Remove the fixture, or explicitly export it if it is a public fixture API.
    test_health.py:12:5: warning[unused-fixture]: Fixture `unused` has no direct or transitive consumer in the full configured suite
      help: Remove the fixture, or explicitly export it if it is a public fixture API.
    4 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
    assert_cmd_snapshot!(context.karva_command_in(context.root()).args(["health", "test_health.py"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    0 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
}

#[test]
fn health_dynamic_fixture_lookup_does_not_claim_unused_fixtures() {
    let context = TestContext::with_file(
        "test_health.py",
        r"
import karva
@karva.fixture
def data(): pass
def test_dynamic(request): request.getfixturevalue('data')
",
    );
    assert_cmd_snapshot!(context.karva_command_in(context.root()).args(["health", "--ignore=missing-fixture"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    0 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
}

#[test]
fn health_exported_and_configuration_selected_fixtures_are_consumed() {
    let context = TestContext::with_file(
        "test_health.py",
        r#"
import karva
__all__ = ["public"]
@karva.fixture
def public(): pass
@karva.fixture
def side_effect(): pass
@karva.tags.use_fixtures("side_effect")
def test_used(): pass
"#,
    );
    assert_cmd_snapshot!(context.karva_command_in(context.root()).arg("health"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    0 health finding(s) (source-only; tests were not executed)

    ----- stderr -----
    ");
}
