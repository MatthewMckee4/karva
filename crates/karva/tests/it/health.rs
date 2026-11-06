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
