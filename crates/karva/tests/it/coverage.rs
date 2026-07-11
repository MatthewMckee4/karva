use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

fn normalize_xml_report(xml: &str) -> String {
    let start = xml.find("<coverage version=\"1.0\" timestamp=\"").unwrap();
    let end = xml[start..]
        .find(" lines-valid=")
        .map(|offset| start + offset)
        .unwrap();

    let mut normalized = xml.to_string();
    normalized.replace_range(
        start..end,
        "<coverage version=\"1.0\" timestamp=\"[TIMESTAMP]\"",
    );
    normalized
}

#[test]
fn test_no_cov_no_coverage_table() {
    let context = TestContext::with_file(
        "test_simple.py",
        r"
def test_one():
    assert 1 + 1 == 2
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--status-level=none")
            .arg("test_simple.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_control_flow() {
    let context = TestContext::with_file(
        "test_control.py",
        r"
def both_branches(x):
    if x > 0:
        return 'pos'
    return 'neg'

def with_loop(n):
    total = 0
    for i in range(n):
        total += i
    return total

def with_try():
    try:
        return 'ok'
    except Exception:
        return 'err'

def test_pos():
    assert both_branches(1) == 'pos'
    assert with_loop(2) == 1
    assert with_try() == 'ok'
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--status-level=none")
            .arg("test_control.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name              Stmts   Miss   Cover
    [LONG-LINE]
    test_control.py      18      3     83%
    [LONG-LINE]
    TOTAL                18      3     83%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_branch_reports_partial_branch() {
    let context = TestContext::with_file(
        "test_branch.py",
        r"
def choose(flag):
    if flag:
        return 'yes'
    return 'no'

def test_yes():
    assert choose(True) == 'yes'
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--cov-branch")
            .arg("--cov-report=term-missing")
            .arg("--status-level=none")
            .arg("test_branch.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name             Stmts   Miss   Branch   BrPart   Cover   Missing
    [LONG-LINE]
    test_branch.py       6      1        2        1     75%   5
    [LONG-LINE]
    TOTAL                6      1        2        1     75%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_branch_json_reports_branch_arcs() {
    let context = TestContext::with_file(
        "test_branch_json.py",
        r"
def choose(flag):
    if flag:
        return 'yes'
    return 'no'

def test_yes():
    assert choose(True) == 'yes'
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--cov-branch")
            .arg("--cov-report=json")
            .arg("--status-level=none")
            .arg("test_branch_json.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );

    let json = context.read_file("coverage.json");
    insta::assert_snapshot!(json, @r#"
    {
      "meta": {
        "format": 2,
        "version": "karva"
      },
      "files": {
        "test_branch_json.py": {
          "executed_lines": [
            2,
            3,
            4,
            7,
            8
          ],
          "summary": {
            "covered_lines": 5,
            "num_statements": 6,
            "percent_covered": 75.0,
            "missing_lines": [
              5
            ],
            "excluded_lines": [],
            "num_branches": 2,
            "num_partial_branches": 1,
            "covered_branches": 1,
            "missing_branches": 1,
            "percent_branches_covered": 50.0
          },
          "missing_lines": [
            5
          ],
          "excluded_lines": [],
          "executed_branches": [
            [
              3,
              4
            ]
          ],
          "missing_branches": [
            [
              3,
              5
            ]
          ]
        }
      },
      "totals": {
        "covered_lines": 5,
        "num_statements": 6,
        "percent_covered": 75.0,
        "num_branches": 2,
        "num_partial_branches": 1,
        "covered_branches": 1,
        "missing_branches": 1,
        "percent_branches_covered": 50.0
      }
    }
    "#);
}

#[test]
fn test_cov_branch_can_be_enabled_from_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.coverage]
sources = [""]
branch = true
"#,
        ),
        (
            "test_branch_config.py",
            r"
def choose(flag):
    if flag:
        return 'yes'
    return 'no'

def test_yes():
    assert choose(True) == 'yes'
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov-report=term-missing")
            .arg("--status-level=none")
            .arg("test_branch_config.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name                    Stmts   Miss   Branch   BrPart   Cover   Missing
    [LONG-LINE]
    test_branch_config.py       6      1        2        1     75%   5
    [LONG-LINE]
    TOTAL                       6      1        2        1     75%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_saves_on_test_failure() {
    let context = TestContext::with_file(
        "test_failing.py",
        r"
def helper():
    return 1

def test_pass():
    assert helper() == 1

def test_fail():
    assert helper() == 999
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--status-level=none")
            .arg("--final-status-level=none")
            .arg("test_failing.py"),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    Name              Stmts   Miss   Cover
    [LONG-LINE]
    test_failing.py       6      0    100%
    [LONG-LINE]
    TOTAL                 6      0    100%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_multiple_sources() {
    let context = TestContext::with_files([
        ("pkg_a/__init__.py", ""),
        ("pkg_a/code.py", "def a():\n    return 1\n"),
        ("pkg_b/__init__.py", ""),
        ("pkg_b/code.py", "def b():\n    return 2\n"),
        ("pkg_c/__init__.py", ""),
        ("pkg_c/code.py", "def c():\n    return 3\n"),
        (
            "test_multi.py",
            r"
from pkg_a.code import a
from pkg_b.code import b
from pkg_c.code import c

def test_all():
    assert a() + b() + c() == 6
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov=pkg_a")
            .arg("--cov=pkg_b")
            .arg("--status-level=none")
            .arg("test_multi.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name            Stmts   Miss   Cover
    [LONG-LINE]
    pkg_a/code.py       2      0    100%
    pkg_b/code.py       2      0    100%
    [LONG-LINE]
    TOTAL               4      0    100%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_basic() {
    let context = TestContext::with_file(
        "test_covered.py",
        r"
def add(a, b):
    return a + b

def test_add():
    assert add(1, 2) == 3
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--status-level=none")
            .arg("test_covered.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name              Stmts   Miss   Cover
    [LONG-LINE]
    test_covered.py       4      0    100%
    [LONG-LINE]
    TOTAL                 4      0    100%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_partial() {
    let context = TestContext::with_file(
        "test_partial.py",
        r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--status-level=none")
            .arg("test_partial.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name              Stmts   Miss   Cover
    [LONG-LINE]
    test_partial.py       6      1     83%
    [LONG-LINE]
    TOTAL                 6      1     83%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_report_term_missing() {
    let context = TestContext::with_file(
        "test_missing.py",
        r"
def covered():
    return 1

def uncovered_a():
    return 2

def uncovered_b():
    x = 3
    y = 4
    return x + y

def test_only_covered():
    assert covered() == 1
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--cov-report=term-missing")
            .arg("--status-level=none")
            .arg("test_missing.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name              Stmts   Miss   Cover   Missing
    [LONG-LINE]
    test_missing.py      10      4     60%   6, 9-11
    [LONG-LINE]
    TOTAL                10      4     60%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_report_term_default_no_missing_column() {
    let context = TestContext::with_file(
        "test_partial.py",
        r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--cov-report=term")
            .arg("--status-level=none")
            .arg("test_partial.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name              Stmts   Miss   Cover
    [LONG-LINE]
    test_partial.py       6      1     83%
    [LONG-LINE]
    TOTAL                 6      1     83%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_report_xml_writes_default_file() {
    let context = TestContext::with_file(
        "test_partial.py",
        r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("--cov")
            .arg("--cov-report=xml")
            .arg("--status-level=none")
            .arg("test_partial.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );

    let xml = normalize_xml_report(&context.read_file("coverage.xml"));
    insta::assert_snapshot!(xml, @r#"
    <?xml version="1.0" ?>
    <coverage version="1.0" timestamp="[TIMESTAMP]" lines-valid="6" lines-covered="5" line-rate="0.8333" branches-covered="0" branches-valid="0" branch-rate="0.0000" complexity="0.0">
      <sources>
        <source><temp_dir>/</source>
      </sources>
      <packages>
        <package name="." line-rate="0.8333" branch-rate="0.0000" complexity="0.0">
          <classes>
            <class name="test_partial.py" filename="test_partial.py" line-rate="0.8333" branch-rate="0.0000" complexity="0.0">
              <methods/>
              <lines>
                <line number="2" hits="1" branch="false"/>
                <line number="3" hits="1" branch="false"/>
                <line number="5" hits="1" branch="false"/>
                <line number="6" hits="0" branch="false"/>
                <line number="8" hits="1" branch="false"/>
                <line number="9" hits="1" branch="false"/>
              </lines>
            </class>
          </classes>
        </package>
      </packages>
    </coverage>
    "#);
}

#[test]
fn test_cov_report_xml_with_custom_path_from_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.coverage]
sources = [""]
report = "xml"
report-path = "build/coverage.xml"
"#,
        ),
        (
            "test_partial.py",
            r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("--status-level=none")
            .arg("test_partial.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );

    let xml = normalize_xml_report(&context.read_file("build/coverage.xml"));
    insta::assert_snapshot!(xml, @r#"
    <?xml version="1.0" ?>
    <coverage version="1.0" timestamp="[TIMESTAMP]" lines-valid="6" lines-covered="5" line-rate="0.8333" branches-covered="0" branches-valid="0" branch-rate="0.0000" complexity="0.0">
      <sources>
        <source><temp_dir>/</source>
      </sources>
      <packages>
        <package name="." line-rate="0.8333" branch-rate="0.0000" complexity="0.0">
          <classes>
            <class name="test_partial.py" filename="test_partial.py" line-rate="0.8333" branch-rate="0.0000" complexity="0.0">
              <methods/>
              <lines>
                <line number="2" hits="1" branch="false"/>
                <line number="3" hits="1" branch="false"/>
                <line number="5" hits="1" branch="false"/>
                <line number="6" hits="0" branch="false"/>
                <line number="8" hits="1" branch="false"/>
                <line number="9" hits="1" branch="false"/>
              </lines>
            </class>
          </classes>
        </package>
      </packages>
    </coverage>
    "#);
}

#[test]
fn test_cov_report_json_writes_default_file() {
    let context = TestContext::with_file(
        "test_partial.py",
        r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("--cov")
            .arg("--cov-report=json")
            .arg("--status-level=none")
            .arg("test_partial.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );

    let json = context.read_file("coverage.json");
    insta::assert_snapshot!(json, @r#"
    {
      "meta": {
        "format": 2,
        "version": "karva"
      },
      "files": {
        "test_partial.py": {
          "executed_lines": [
            2,
            3,
            5,
            8,
            9
          ],
          "summary": {
            "covered_lines": 5,
            "num_statements": 6,
            "percent_covered": 83.33333333333334,
            "missing_lines": [
              6
            ],
            "excluded_lines": []
          },
          "missing_lines": [
            6
          ],
          "excluded_lines": []
        }
      },
      "totals": {
        "covered_lines": 5,
        "num_statements": 6,
        "percent_covered": 83.33333333333334
      }
    }
    "#);
}

#[test]
fn test_cov_writes_coveragepy_sqlite_file() {
    let context = TestContext::with_files([
        (
            "src/app.py",
            "def covered():\n    return 1\n\ndef missed():\n    return 2\n",
        ),
        (
            "test_sqlite.py",
            r"
import sys, os
sys.path.insert(0, os.path.dirname(__file__))
from src.app import covered

def test_covered():
    assert covered() == 1
",
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("--cov=src")
            .arg("--cov-report=json")
            .arg("--status-level=none")
            .arg("test_sqlite.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );

    let bytes = std::fs::read(context.root().join(".coverage")).expect("read .coverage");
    assert!(bytes.starts_with(b"SQLite format 3\0"));
}

#[test]
fn test_cov_context_test_records_json_contexts() {
    let context = TestContext::with_files([
        (
            "src/app.py",
            "def shared():\n    value = 1\n    return value\n\ndef only_one():\n    return shared() + 1\n\ndef only_two():\n    return shared() + 2\n",
        ),
        (
            "test_context.py",
            r"
import sys, os
sys.path.insert(0, os.path.dirname(__file__))
from src.app import only_one, only_two

def test_one():
    assert only_one() == 2

def test_two():
    assert only_two() == 3
",
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("--cov=src")
            .arg("--cov-context=test")
            .arg("--cov-report=json")
            .arg("--status-level=none")
            .arg("test_context.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    "
    );

    let json = context.read_file("coverage.json");
    insta::assert_snapshot!(json, @r#"
    {
      "meta": {
        "format": 2,
        "version": "karva",
        "show_contexts": true
      },
      "files": {
        "src/app.py": {
          "executed_lines": [
            1,
            2,
            3,
            5,
            6,
            8,
            9
          ],
          "summary": {
            "covered_lines": 7,
            "num_statements": 7,
            "percent_covered": 100.0,
            "missing_lines": [],
            "excluded_lines": []
          },
          "missing_lines": [],
          "excluded_lines": [],
          "contexts": {
            "2": [
              "test_context::test_one",
              "test_context::test_two"
            ],
            "3": [
              "test_context::test_one",
              "test_context::test_two"
            ],
            "6": [
              "test_context::test_one"
            ],
            "9": [
              "test_context::test_two"
            ]
          }
        }
      },
      "totals": {
        "covered_lines": 7,
        "num_statements": 7,
        "percent_covered": 100.0
      }
    }
    "#);
}

#[test]
fn test_cov_report_cli_override_uses_default_path_when_config_path_is_stale() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.coverage]
sources = [""]
report = "xml"
report-path = "build/coverage.xml"
"#,
        ),
        (
            "test_partial.py",
            r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("--cov-report=json")
            .arg("--status-level=none")
            .arg("test_partial.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );

    let json = context.read_file("coverage.json");
    insta::assert_snapshot!(json, @r#"
    {
      "meta": {
        "format": 2,
        "version": "karva"
      },
      "files": {
        "test_partial.py": {
          "executed_lines": [
            2,
            3,
            5,
            8,
            9
          ],
          "summary": {
            "covered_lines": 5,
            "num_statements": 6,
            "percent_covered": 83.33333333333334,
            "missing_lines": [
              6
            ],
            "excluded_lines": []
          },
          "missing_lines": [
            6
          ],
          "excluded_lines": []
        }
      },
      "totals": {
        "covered_lines": 5,
        "num_statements": 6,
        "percent_covered": 83.33333333333334
      }
    }
    "#);
    assert!(!context.root().join("build/coverage.xml").exists());
}

#[test]
fn test_cov_report_default_path_is_project_root_from_subdirectory() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.coverage]
sources = [""]
report = "json"
"#,
        ),
        (
            "tests/test_partial.py",
            r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
        ),
    ]);

    let mut command = context.karva_command_in(context.root().join("tests"));
    command
        .arg("test")
        .arg("--no-parallel")
        .arg("--status-level=none")
        .arg("tests/test_partial.py");

    assert_cmd_snapshot!(command, @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let json = context.read_file("coverage.json");
    insta::assert_snapshot!(json, @r#"
    {
      "meta": {
        "format": 2,
        "version": "karva"
      },
      "files": {
        "tests/test_partial.py": {
          "executed_lines": [
            2,
            3,
            5,
            8,
            9
          ],
          "summary": {
            "covered_lines": 5,
            "num_statements": 6,
            "percent_covered": 83.33333333333334,
            "missing_lines": [
              6
            ],
            "excluded_lines": []
          },
          "missing_lines": [
            6
          ],
          "excluded_lines": []
        }
      },
      "totals": {
        "covered_lines": 5,
        "num_statements": 6,
        "percent_covered": 83.33333333333334
      }
    }
    "#);
    assert!(!context.root().join("tests/coverage.json").exists());
}

#[test]
fn test_cov_report_cli_path_is_project_root_relative_from_subdirectory() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.coverage]
sources = [""]
"#,
        ),
        (
            "tests/test_partial.py",
            r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
        ),
    ]);

    let mut command = context.karva_command_in(context.root().join("tests"));
    command
        .arg("test")
        .arg("--no-parallel")
        .arg("--cov-report=json:build/coverage.json")
        .arg("--status-level=none")
        .arg("tests/test_partial.py");

    assert_cmd_snapshot!(command, @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let json = context.read_file("build/coverage.json");
    insta::assert_snapshot!(json, @r#"
    {
      "meta": {
        "format": 2,
        "version": "karva"
      },
      "files": {
        "tests/test_partial.py": {
          "executed_lines": [
            2,
            3,
            5,
            8,
            9
          ],
          "summary": {
            "covered_lines": 5,
            "num_statements": 6,
            "percent_covered": 83.33333333333334,
            "missing_lines": [
              6
            ],
            "excluded_lines": []
          },
          "missing_lines": [
            6
          ],
          "excluded_lines": []
        }
      },
      "totals": {
        "covered_lines": 5,
        "num_statements": 6,
        "percent_covered": 83.33333333333334
      }
    }
    "#);
    assert!(!context.root().join("tests/build/coverage.json").exists());
}

#[test]
fn test_cov_report_html_with_custom_dir_from_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.coverage]
sources = [""]
report = "html"
report-path = "build/htmlcov"
"#,
        ),
        (
            "test_partial.py",
            r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("--status-level=none")
            .arg("test_partial.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );

    let html = context.read_file("build/htmlcov/index.html");
    insta::assert_snapshot!(html, @r#"
    <!DOCTYPE html>
    <html lang="en">
    <head>
      <meta charset="utf-8">
      <meta name="viewport" content="width=device-width, initial-scale=1">
      <title>Coverage report</title>
      <style>body{font-family:system-ui,sans-serif;margin:2rem;}table{border-collapse:collapse;width:100%;}th,td{padding:.5rem;border-bottom:1px solid #ddd;text-align:left;}td.num{text-align:right;font-variant-numeric:tabular-nums;}code{font-family:ui-monospace,SFMono-Regular,monospace;}thead{background:#f5f5f5;}h1{margin-top:0;}</style>
    </head>
    <body>
      <h1>Coverage report</h1>
      <p>Total coverage: <strong>83%</strong> (5/6)</p>
      <table>
        <thead>
          <tr><th>Name</th><th>Stmts</th><th>Miss</th><th>Cover</th><th>Missing</th></tr>
        </thead>
        <tbody>
          <tr><td><code>test_partial.py</code></td><td class="num">6</td><td class="num">1</td><td class="num">83%</td><td><code>6</code></td></tr>
          <tr><td><strong>TOTAL</strong></td><td class="num"><strong>6</strong></td><td class="num"><strong>1</strong></td><td class="num"><strong>83%</strong></td><td></td></tr>
        </tbody>
      </table>
    </body>
    </html>
    "#);
}

#[test]
fn test_cov_report_path_warns_for_term_report_from_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.coverage]
sources = [""]
report = "term"
report-path = "build/coverage.xml"
"#,
        ),
        (
            "test_partial.py",
            r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("--status-level=none")
            .arg("test_partial.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped
    warning: `coverage.report-path` is ignored when `coverage.report` is `term`

    Name              Stmts   Miss   Cover
    [LONG-LINE]
    test_partial.py       6      1     83%
    [LONG-LINE]
    TOTAL                 6      1     83%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_skips_docstrings() {
    let context = TestContext::with_file(
        "test_docstrings.py",
        r#"
"""Module docstring."""


def doc_only():
    """Function with only a docstring is never called."""


def test_nothing():
    """This docstring should not count as a statement."""
    assert True
"#,
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--status-level=none")
            .arg("test_docstrings.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name                 Stmts   Miss   Cover
    [LONG-LINE]
    test_docstrings.py       3      0    100%
    [LONG-LINE]
    TOTAL                    3      0    100%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_walks_class_bodies() {
    let context = TestContext::with_file(
        "test_class.py",
        r"
class Foo:
    def method(self):
        return 1

    def unused(self):
        return 2

def test_method():
    assert Foo().method() == 1
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--status-level=none")
            .arg("test_class.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name            Stmts   Miss   Cover
    [LONG-LINE]
    test_class.py       7      1     86%
    [LONG-LINE]
    TOTAL               7      1     86%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_combines_across_workers() {
    let context = TestContext::with_files([
        (
            "test_a.py",
            r"
def test_a():
    assert 1 + 1 == 2
",
        ),
        (
            "test_b.py",
            r"
def test_b():
    assert 2 + 2 == 4
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command()
            .arg("--num-workers=2")
            .arg("--cov")
            .arg("--status-level=none"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    Name        Stmts   Miss   Cover
    [LONG-LINE]
    test_a.py       2      0    100%
    test_b.py       2      0    100%
    [LONG-LINE]
    TOTAL           4      0    100%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_with_source_filter() {
    let context = TestContext::with_files([
        (
            "src/mymod.py",
            r"
def add(a, b):
    return a + b
",
        ),
        (
            "test_mymod.py",
            r"
import sys, os
sys.path.insert(0, os.path.dirname(__file__))
from src.mymod import add

def test_add():
    assert add(2, 3) == 5
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov=src")
            .arg("--status-level=none")
            .arg("test_mymod.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name           Stmts   Miss   Cover
    [LONG-LINE]
    src/mymod.py       2      0    100%
    [LONG-LINE]
    TOTAL              2      0    100%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_include_and_omit_filters_report_rows() {
    let context = TestContext::with_files([
        (
            "src/app/service.py",
            r"
def used():
    return 1
",
        ),
        (
            "src/app/generated.py",
            r"
def generated():
    return 2
",
        ),
        (
            "src/other.py",
            r"
def outside():
    return 3
",
        ),
        (
            "test_service.py",
            r"
import sys, os
sys.path.insert(0, os.path.dirname(__file__))
from src.app.service import used

def test_used():
    assert used() == 1
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov=src")
            .arg("--cov-include=src/app/*")
            .arg("--cov-omit=src/app/generated.py")
            .arg("--status-level=none")
            .arg("test_service.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name                 Stmts   Miss   Cover
    [LONG-LINE]
    src/app/service.py       2      0    100%
    [LONG-LINE]
    TOTAL                    2      0    100%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_sources_from_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.coverage]
sources = ["src"]
"#,
        ),
        (
            "src/mymod.py",
            r"
def add(a, b):
    return a + b
",
        ),
        (
            "test_mymod.py",
            r"
import sys, os
sys.path.insert(0, os.path.dirname(__file__))
from src.mymod import add

def test_add():
    assert add(2, 3) == 5
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--status-level=none")
            .arg("test_mymod.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name           Stmts   Miss   Cover
    [LONG-LINE]
    src/mymod.py       2      0    100%
    [LONG-LINE]
    TOTAL              2      0    100%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_include_and_omit_from_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.coverage]
sources = ["src"]
include = ["src/app/*"]
omit = ["src/app/generated.py"]
"#,
        ),
        (
            "src/app/service.py",
            r"
def used():
    return 1
",
        ),
        (
            "src/app/generated.py",
            r"
def generated():
    return 2
",
        ),
        (
            "src/other.py",
            r"
def outside():
    return 3
",
        ),
        (
            "test_service.py",
            r"
import sys, os
sys.path.insert(0, os.path.dirname(__file__))
from src.app.service import used

def test_used():
    assert used() == 1
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--status-level=none")
            .arg("test_service.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name                 Stmts   Miss   Cover
    [LONG-LINE]
    src/app/service.py       2      0    100%
    [LONG-LINE]
    TOTAL                    2      0    100%

    ----- stderr -----
    "
    );
}

#[test]
fn test_no_cov_overrides_config_sources() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.coverage]
sources = [""]
"#,
        ),
        (
            "test_simple.py",
            r"
def test_one():
    assert 1 + 1 == 2
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--no-cov")
            .arg("--status-level=none")
            .arg("test_simple.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
}

/// `--cov --no-cov` (no-cov last) disables coverage; clap `overrides_with`
/// makes the later flag win.
#[test]
fn test_no_cov_after_cov_disables_coverage() {
    let context = TestContext::with_file(
        "test_simple.py",
        r"
def test_one():
    assert 1 + 1 == 2
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--no-cov")
            .arg("--status-level=none")
            .arg("test_simple.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
}

/// `# pragma: no cover` on a simple statement excludes that line from
/// `Stmts`, while a pragma on a compound head excludes the whole block.
/// `uncovered()` would be 1 missed line; the pragma drops it from the
/// statement count entirely so coverage reports 100%.
#[test]
fn test_cov_pragma_no_cover_excludes_block() {
    let context = TestContext::with_file(
        "test_pragma.py",
        r"
def covered():
    return 1

def uncovered():  # pragma: no cover
    return 2

def test_only_covered():
    assert covered() == 1
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--status-level=none")
            .arg("test_pragma.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name             Stmts   Miss   Cover
    [LONG-LINE]
    test_pragma.py       4      0    100%
    [LONG-LINE]
    TOTAL                4      0    100%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_includes_unimported_files_at_zero_percent() {
    let context = TestContext::with_files([
        ("src/__init__.py", ""),
        (
            "src/imported.py",
            r"
def used():
    return 1
",
        ),
        (
            "src/unimported.py",
            r"
def lonely():
    return 2

def other():
    return 3
",
        ),
        (
            "test_partial.py",
            r"
import sys, os
sys.path.insert(0, os.path.dirname(__file__))
from src.imported import used

def test_used():
    assert used() == 1
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov=src")
            .arg("--status-level=none")
            .arg("test_partial.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name                Stmts   Miss   Cover
    [LONG-LINE]
    src/imported.py         2      0    100%
    src/unimported.py       4      4      0%
    [LONG-LINE]
    TOTAL                   6      4     33%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_fail_under_below_threshold_fails() {
    let context = TestContext::with_file(
        "test_partial.py",
        r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--cov-fail-under=90")
            .arg("--status-level=none")
            .arg("test_partial.py"),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name              Stmts   Miss   Cover
    [LONG-LINE]
    test_partial.py       6      1     83%
    [LONG-LINE]
    TOTAL                 6      1     83%

    coverage failure: required total coverage of 90% not reached, total coverage was 83.33%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_fail_under_at_threshold_passes() {
    let context = TestContext::with_file(
        "test_covered.py",
        r"
def add(a, b):
    return a + b

def test_add():
    assert add(1, 2) == 3
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--cov-fail-under=100")
            .arg("--status-level=none")
            .arg("test_covered.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name              Stmts   Miss   Cover
    [LONG-LINE]
    test_covered.py       4      0    100%
    [LONG-LINE]
    TOTAL                 4      0    100%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_fail_under_from_config_below_threshold_fails() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.coverage]
sources = [""]
fail-under = 90
"#,
        ),
        (
            "test_partial.py",
            r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--status-level=none")
            .arg("test_partial.py"),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name              Stmts   Miss   Cover
    [LONG-LINE]
    test_partial.py       6      1     83%
    [LONG-LINE]
    TOTAL                 6      1     83%

    coverage failure: required total coverage of 90% not reached, total coverage was 83.33%

    ----- stderr -----
    "
    );
}

/// CLI `--cov-fail-under` overrides the value set in `karva.toml`.
#[test]
fn test_cov_fail_under_cli_overrides_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.coverage]
sources = [""]
fail-under = 100
"#,
        ),
        (
            "test_partial.py",
            r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov-fail-under=50")
            .arg("--status-level=none")
            .arg("test_partial.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name              Stmts   Miss   Cover
    [LONG-LINE]
    test_partial.py       6      1     83%
    [LONG-LINE]
    TOTAL                 6      1     83%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_fail_under_rejects_out_of_range() {
    let context = TestContext::with_file(
        "test_simple.py",
        r"
def test_one():
    assert 1 + 1 == 2
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--cov-fail-under=150")
            .arg("test_simple.py"),
        @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    error: invalid value '150' for '--cov-fail-under <PERCENT>': must be between 0 and 100, got `150`

    For more information, try '--help'.
    "
    );
}

/// Regression test for <https://github.com/MatthewMckee4/karva/issues/760>.
///
/// On Python 3.12+ the `sys.monitoring` LINE callback (and on older versions
/// the `sys.settrace` callback) can fire on threads other than the one that
/// installed the tracer. The `PyO3` `#[pyclass]` must therefore be safe to
/// access cross-thread; otherwise `unsendable` triggers a panic of the form
/// `CoverageTracer is unsendable, but sent to another thread`.
#[test]
fn test_cov_traces_python_threads() {
    let context = TestContext::with_file(
        "test_threaded.py",
        r"
import threading

def in_thread():
    x = 1
    y = x + 1
    return y

def test_thread_runs():
    results = []
    threads = [
        threading.Thread(target=lambda: results.append(in_thread()))
        for _ in range(4)
    ]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert results == [2, 2, 2, 2]
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--cov")
            .arg("--status-level=none")
            .arg("test_threaded.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name               Stmts   Miss   Cover
    [LONG-LINE]
    test_threaded.py      13      0    100%
    [LONG-LINE]
    TOTAL                 13      0    100%

    ----- stderr -----
    "
    );
}

#[test]
fn test_cov_report_term_missing_from_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.coverage]
sources = [""]
report = "term-missing"
"#,
        ),
        (
            "test_missing.py",
            r"
def covered():
    return 1

def uncovered():
    return 2

def test_only_covered():
    assert covered() == 1
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel()
            .arg("--status-level=none")
            .arg("test_missing.py"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    Name              Stmts   Miss   Cover   Missing
    [LONG-LINE]
    test_missing.py       6      1     83%   6
    [LONG-LINE]
    TOTAL                 6      1     83%

    ----- stderr -----
    "
    );
}
