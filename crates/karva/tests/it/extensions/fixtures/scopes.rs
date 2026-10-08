use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[rstest]
fn test_fixture_module_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "conftest.py",
            format!(
                r"
                    import {framework}

                    call_count = []

                    @{framework}.fixture(scope='module')
                    def module_fixture():
                        call_count.append(1)
                        return 'MODULE'
                "
            )
            .as_str(),
        ),
        (
            "test.py",
            r"
                    from conftest import call_count

                    def test_first(module_fixture):
                        assert module_fixture == 'MODULE'

                    def test_second(module_fixture):
                        assert module_fixture == 'MODULE'
                        # Module scope means fixture is called once
                        assert len(call_count) == 1
                ",
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test::test_first(module_fixture='MODULE')
                PASS [TIME] test::test_second(module_fixture='MODULE')
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_session_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "conftest.py",
            format!(
                r"
                    import {framework}

                    call_count = []

                    @{framework}.fixture(scope='session')
                    def session_fixture():
                        call_count.append(1)
                        return len(call_count)
                "
            )
            .as_str(),
        ),
        (
            "test_1.py",
            r"
                    def test_a1(session_fixture):
                        assert session_fixture == 1

                    def test_a2(session_fixture):
                        assert session_fixture == 1
                ",
        ),
        (
            "test_2.py",
            r"
                    def test_b1(session_fixture):
                        assert session_fixture == 1
                ",
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 3 tests across 1 worker
                PASS [TIME] test_1::test_a1(session_fixture=1)
                PASS [TIME] test_1::test_a2(session_fixture=1)
                PASS [TIME] test_2::test_b1(session_fixture=1)
        ────────────
             Summary [TIME] 3 tests run: 3 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_package_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "package/conftest.py",
            format!(
                r"
                    import {framework}

                    calls = []

                    @{framework}.fixture(scope='package')
                    def package_fixture():
                        calls.append(1)
                        return len(calls)
                "
            )
            .as_str(),
        ),
        (
            "package/test_one.py",
            r"
                    def test_in_one(package_fixture):
                        assert package_fixture == 1
                ",
        ),
        (
            "package/test_two.py",
            r"
                    def test_in_two(package_fixture):
                        assert package_fixture == 1
                ",
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] package.test_one::test_in_one(package_fixture=1)
                PASS [TIME] package.test_two::test_in_two(package_fixture=1)
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_module_fixture_is_recreated_between_modules(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "conftest.py",
            format!(
                r#"
import {framework}

calls = []

@{framework}.fixture(scope="module")
def value():
    calls.append("setup")
    return len(calls)
"#
            )
            .as_str(),
        ),
        ("test_first.py", "def test_value(value): assert value == 1"),
        ("test_second.py", "def test_value(value): assert value == 2"),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test_first::test_value(value=1)
                PASS [TIME] test_second::test_value(value=2)
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_none_fixture_value_is_cached(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
import {framework}

calls = []

@{framework}.fixture(scope="module")
def value():
    calls.append("setup")
    return None

def test_first(value):
    assert value is None
    assert calls == ["setup"]

def test_second(value):
    assert value is None
    assert calls == ["setup"]
"#
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test::test_first(value=None)
                PASS [TIME] test::test_second(value=None)
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_session_fixture_finalizes_once_after_all_modules(
    #[values("pytest", "karva")] framework: &str,
) {
    let context = TestContext::with_files([
        (
            "conftest.py",
            format!(
                r#"
from pathlib import Path
import {framework}

LOG = Path("lifecycle.log")
LOG.write_text("")

@{framework}.fixture(scope="session")
def resource():
    with LOG.open("a") as stream:
        stream.write("setup\n")
    yield "resource"
    with LOG.open("a") as stream:
        stream.write("teardown\n")
"#
            )
            .as_str(),
        ),
        (
            "test_first.py",
            r#"
from pathlib import Path

def test_resource(resource):
    assert resource == "resource"
    assert Path("lifecycle.log").read_text() == "setup\n"
"#,
        ),
        (
            "test_second.py",
            r#"
from pathlib import Path

def test_resource(resource):
    assert resource == "resource"
    assert Path("lifecycle.log").read_text() == "setup\n"
"#,
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test_first::test_resource(resource='resource')
                PASS [TIME] test_second::test_resource(resource='resource')
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
    assert_eq!(
        context
            .read_file("lifecycle.log")
            .lines()
            .collect::<Vec<_>>(),
        ["setup", "teardown"]
    );
}

#[rstest]
fn test_module_fixture_finalizes_before_next_module(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "conftest.py",
            format!(
                r#"
from pathlib import Path
import {framework}

LOG = Path("lifecycle.log")
LOG.write_text("")

@{framework}.fixture(scope="module")
def resource():
    with LOG.open("a") as stream:
        stream.write("setup\n")
    yield "resource"
    with LOG.open("a") as stream:
        stream.write("teardown\n")
"#
            )
            .as_str(),
        ),
        (
            "test_first.py",
            r#"
from pathlib import Path

def test_resource(resource):
    assert Path("lifecycle.log").read_text() == "setup\n"
"#,
        ),
        (
            "test_second.py",
            r#"
from pathlib import Path

def test_resource(resource):
    assert Path("lifecycle.log").read_text() == "setup\nteardown\nsetup\n"
"#,
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test_first::test_resource(resource='resource')
                PASS [TIME] test_second::test_resource(resource='resource')
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
    assert_eq!(
        context
            .read_file("lifecycle.log")
            .lines()
            .collect::<Vec<_>>(),
        ["setup", "teardown", "setup", "teardown"]
    );
}
