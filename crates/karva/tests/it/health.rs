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
