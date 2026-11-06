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
            "byte_range": [
              31,
              40
            ],
            "code": "shadowed-definition",
            "message": "`test_same` is shadowed by the later definition, which wins. Rename or remove one definition",
            "path": "test_health.py",
            "related": [
              {
                "byte_range": [
                  53,
                  62
                ],
                "message": "Later definition replaces the earlier binding",
                "path": "test_health.py"
              }
            ]
          },
          {
            "byte_range": [
              96,
              107
            ],
            "code": "shadowed-definition",
            "message": "`test_method` is shadowed by the later definition, which wins. Rename or remove one definition",
            "path": "test_health.py",
            "related": [
              {
                "byte_range": [
                  125,
                  136
                ],
                "message": "Later definition replaces the earlier binding",
                "path": "test_health.py"
              }
            ]
          },
          {
            "byte_range": [
              163,
              167
            ],
            "code": "shadowed-definition",
            "message": "`data` is shadowed by the later definition, which wins. Rename or remove one definition",
            "path": "test_health.py",
            "related": [
              {
                "byte_range": [
                  180,
                  184
                ],
                "message": "Later definition replaces the earlier binding",
                "path": "test_health.py"
              }
            ]
          },
          {
            "byte_range": [
              209,
              225
            ],
            "code": "shadowed-definition",
            "message": "`test_conditional` may be shadowed by the later definition if both execute; the later definition wins. Rename or remove one definition",
            "path": "test_health.py",
            "related": [
              {
                "byte_range": [
                  250,
                  266
                ],
                "message": "Later definition replaces the earlier binding",
                "path": "test_health.py"
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
