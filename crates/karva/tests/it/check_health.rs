use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[rstest]
fn check_health_reports_shadowed_definitions_without_importing(
    #[values("text", "json")] format: &str,
) {
    let context = TestContext::with_file(
        "test_health.py",
        r"from karva import fixture

def test_same(): pass
def test_same(): pass

class TestExample:
 def test_method(self): pass
 def test_method(self): pass

@fixture
def data(): pass
def data(): pass

if first:
 def test_conditional(): pass
if second:
 def test_conditional(): pass

raise RuntimeError('must not import')
",
    );
    let mut command = context.karva_command_in(context.root());
    command.args(["check", "--output-format", format]);
    if format == "text" {
        assert_cmd_snapshot!(command, @"
        success: false
        exit_code: 1
        ----- stdout -----
        warning[shadowed-definition]: `test_same` is shadowed by the later definition, which wins. Rename or remove one definition
         --> test_health.py:3:5
          |
        3 | def test_same(): pass
          |     ^^^^^^^^^
         ::: test_health.py:4:5
          |
        4 | def test_same(): pass
          |     --------- Later definition replaces the earlier binding

        warning[shadowed-definition]: `test_method` is shadowed by the later definition, which wins. Rename or remove one definition
         --> test_health.py:7:6
          |
        7 |  def test_method(self): pass
          |      ^^^^^^^^^^^
         ::: test_health.py:8:6
          |
        8 |  def test_method(self): pass
          |      ----------- Later definition replaces the earlier binding

        warning[shadowed-definition]: `data` is shadowed by the later definition, which wins. Rename or remove one definition
          --> test_health.py:11:5
           |
        11 | def data(): pass
           |     ^^^^
          ::: test_health.py:12:5
           |
        12 | def data(): pass
           |     ---- Later definition replaces the earlier binding

        warning[shadowed-definition]: `test_conditional` may be shadowed by the later definition if both execute; the later definition wins. Rename or remove one definition
          --> test_health.py:15:6
           |
        15 |  def test_conditional(): pass
           |      ^^^^^^^^^^^^^^^^
          ::: test_health.py:17:6
           |
        17 |  def test_conditional(): pass
           |      ---------------- Later definition replaces the earlier binding


        ----- stderr -----
        ");
    } else {
        assert_cmd_snapshot!(command, @r#"
        success: false
        exit_code: 1
        ----- stdout -----
        [
          {
            "code": "shadowed-definition",
            "message": "`test_same` is shadowed by the later definition, which wins. Rename or remove one definition",
            "path": "test_health.py",
            "byte_range": [
              31,
              40
            ],
            "start": {
              "line": 3,
              "column": 5
            },
            "end": {
              "line": 3,
              "column": 14
            },
            "related": [
              {
                "message": "Later definition replaces the earlier binding",
                "path": "test_health.py",
                "byte_range": [
                  53,
                  62
                ],
                "start": {
                  "line": 4,
                  "column": 5
                },
                "end": {
                  "line": 4,
                  "column": 14
                }
              }
            ]
          },
          {
            "code": "shadowed-definition",
            "message": "`test_method` is shadowed by the later definition, which wins. Rename or remove one definition",
            "path": "test_health.py",
            "byte_range": [
              96,
              107
            ],
            "start": {
              "line": 7,
              "column": 6
            },
            "end": {
              "line": 7,
              "column": 17
            },
            "related": [
              {
                "message": "Later definition replaces the earlier binding",
                "path": "test_health.py",
                "byte_range": [
                  125,
                  136
                ],
                "start": {
                  "line": 8,
                  "column": 6
                },
                "end": {
                  "line": 8,
                  "column": 17
                }
              }
            ]
          },
          {
            "code": "shadowed-definition",
            "message": "`data` is shadowed by the later definition, which wins. Rename or remove one definition",
            "path": "test_health.py",
            "byte_range": [
              163,
              167
            ],
            "start": {
              "line": 11,
              "column": 5
            },
            "end": {
              "line": 11,
              "column": 9
            },
            "related": [
              {
                "message": "Later definition replaces the earlier binding",
                "path": "test_health.py",
                "byte_range": [
                  180,
                  184
                ],
                "start": {
                  "line": 12,
                  "column": 5
                },
                "end": {
                  "line": 12,
                  "column": 9
                }
              }
            ]
          },
          {
            "code": "shadowed-definition",
            "message": "`test_conditional` may be shadowed by the later definition if both execute; the later definition wins. Rename or remove one definition",
            "path": "test_health.py",
            "byte_range": [
              209,
              225
            ],
            "start": {
              "line": 15,
              "column": 6
            },
            "end": {
              "line": 15,
              "column": 22
            },
            "related": [
              {
                "message": "Later definition replaces the earlier binding",
                "path": "test_health.py",
                "byte_range": [
                  250,
                  266
                ],
                "start": {
                  "line": 17,
                  "column": 6
                },
                "end": {
                  "line": 17,
                  "column": 22
                }
              }
            ]
          }
        ]

        ----- stderr -----
        "#);
    }
}

#[test]
fn check_health_accepts_distinct_names_and_exclusive_branches() {
    let context = TestContext::with_file(
        "test_health.py",
        "raise RuntimeError('must not import')\nif dynamic:\n def test_same(): pass\nelse:\n def test_same(): pass\n",
    );
    let mut command = context.karva_command_in(context.root());
    command.args(["check", "--output-format", "json"]);
    assert_cmd_snapshot!(command, @"
    success: true
    exit_code: 0
    ----- stdout -----
    []

    ----- stderr -----
    ");
}

#[test]
fn check_health_uses_configured_paths_profiles_and_ignore_files() {
    let context = TestContext::with_files([
        (
            "settings.toml",
            "[profile.default.src]\ninclude = ['suite']\n[profile.ci.test]\ntest-function-prefix = 'spec'\n",
        ),
        ("suite/a.py", "def spec_é(): pass\ndef spec_é(): pass\n"),
        (
            "suite/nested/conftest.py",
            "from karva import fixture\n@fixture\ndef data(): pass\ndef data(): pass\n",
        ),
        (
            "suite/ignored.py",
            "def spec_ignored(): pass\ndef spec_ignored(): pass\n",
        ),
        ("suite/.gitignore", "ignored.py\n"),
        (
            "outside.py",
            "def spec_outside(): pass\ndef spec_outside(): pass\n",
        ),
    ]);
    let mut command = context.karva_command_in(context.root());
    command.args([
        "check",
        "--config-file",
        "settings.toml",
        "--profile",
        "ci",
        "--output-format",
        "json",
    ]);
    assert_cmd_snapshot!(command, @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    [
      {
        "code": "shadowed-definition",
        "message": "`spec_é` is shadowed by the later definition, which wins. Rename or remove one definition",
        "path": "suite/a.py",
        "byte_range": [
          4,
          11
        ],
        "start": {
          "line": 1,
          "column": 5
        },
        "end": {
          "line": 1,
          "column": 11
        },
        "related": [
          {
            "message": "Later definition replaces the earlier binding",
            "path": "suite/a.py",
            "byte_range": [
              24,
              31
            ],
            "start": {
              "line": 2,
              "column": 5
            },
            "end": {
              "line": 2,
              "column": 11
            }
          }
        ]
      },
      {
        "code": "shadowed-definition",
        "message": "`data` is shadowed by the later definition, which wins. Rename or remove one definition",
        "path": "suite/nested/conftest.py",
        "byte_range": [
          39,
          43
        ],
        "start": {
          "line": 3,
          "column": 5
        },
        "end": {
          "line": 3,
          "column": 9
        },
        "related": [
          {
            "message": "Later definition replaces the earlier binding",
            "path": "suite/nested/conftest.py",
            "byte_range": [
              56,
              60
            ],
            "start": {
              "line": 4,
              "column": 5
            },
            "end": {
              "line": 4,
              "column": 9
            }
          }
        ]
      }
    ]

    ----- stderr -----
    "#);
}

#[rstest]
fn check_health_accepts_explicit_file_and_nested_directory(
    #[values("suite/test_a.py", "suite/nested")] path: &str,
) {
    let context = TestContext::with_files([
        ("suite/test_a.py", "def test_clean(): pass\n"),
        ("suite/nested/test_b.py", "def test_other(): pass\n"),
        (
            "test_outside.py",
            "def test_same(): pass\ndef test_same(): pass\n",
        ),
    ]);
    let mut command = context.karva_command_in(context.root());
    command.args(["check", path, "--output-format", "json"]);
    insta::allow_duplicates! {
    assert_cmd_snapshot!(command, @"
    success: true
    exit_code: 0
    ----- stdout -----
    []

    ----- stderr -----
    ");
    }
}

#[test]
fn check_health_defaults_to_tests_directory() {
    let context = TestContext::with_files([
        ("tests/test_clean.py", "def test_clean(): pass\n"),
        (
            "test_outside.py",
            "def test_same(): pass\ndef test_same(): pass\n",
        ),
    ]);
    let mut command = context.karva_command_in(context.root());
    command.args(["check", "--output-format", "json"]);
    assert_cmd_snapshot!(command, @"
    success: true
    exit_code: 0
    ----- stdout -----
    []

    ----- stderr -----
    ");
}

#[test]
fn check_health_reports_nonexistent_path() {
    let context = TestContext::new();
    let mut command = context.karva_command_in(context.root());
    command.args(["check", "missing.py"]);
    assert_cmd_snapshot!(command, @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: path `<temp_dir>/missing.py` could not be found
    ");
}
