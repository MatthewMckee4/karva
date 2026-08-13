use regex::Regex;

use crate::common::TestContext;

#[test]
fn full_colored_cli_output() {
    let context = TestContext::with_file(
        "test_output.py",
        r#"
        import karva

        def test_pass():
            pass

        @karva.tags.skip("not today")
        def test_skip():
            pass

        @karva.fixture
        def first():
            return 1

        @karva.fixture
        def second():
            return 2

        @karva.fixture
        def third():
            return 3

        def test_assertion_failure(third, second, first):
            assert 1 == 2

        def test_snapshot_failure():
            karva.assert_snapshot("changed")

        def test_new_snapshot():
            karva.assert_snapshot("new")

        def test_error(missing_fixture):
            pass
        "#,
    );
    context.write_file(
        "snapshots/test_output__test_snapshot_failure.snap",
        r"---
source: test_output.py:27::test_snapshot_failure
---
original
",
    );

    let output = context
        .command_no_parallel()
        .args(["--color=always", "--status-level=all"])
        .output()
        .expect("karva should run");
    let actual = format!(
        "success: {}\nexit_code: {}\n----- stdout -----\n{}\n----- stderr -----\n{}",
        output.status.success(),
        output
            .status
            .code()
            .expect("karva should return an exit code"),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let timing = Regex::new(r"\[\s*(\d+m )?(\d+\.)?\d+(ms|s)\]").expect("valid regex");
    let actual = timing.replace_all(&actual, "[TIME]");

    snapbox::assert_data_eq!(
        actual.as_ref(),
        snapbox::file!["snapshots/full_colored_cli_output.term.svg": TermSvg],
    );
}
