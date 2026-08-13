use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn crash_recovery_preserves_work_from_an_unaffected_worker() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path


def record(name):
    Path("completed_" + name).touch()


def test_a():
    record("a")


def test_b():
    record("b")


def test_c():
    record("c")


def test_d():
    os._exit(39)


def test_e():
    record("e")


def test_f():
    record("f")


def test_g():
    record("g")


def test_h():
    record("h")


def test_i():
    record("i")


def test_j():
    record("j")
"#,
    );

    assert_cmd_snapshot!(
        context.command().args([
            "--num-workers=2",
            "--shuffle",
            "--random-seed=170938",
            "--status-level=none",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
    Random seed: [SEED]

    failures:

    test::test_d:

    error[worker-crashed]: Worker terminated with exit code 39 while running `test::test_d`

    ────────────
         Summary [TIME] 10 tests run: 9 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 39 in [TIME]
    "
    );
    for name in ['a', 'b', 'c', 'e', 'f', 'g', 'h', 'i', 'j'] {
        assert!(context.root().join(format!("completed_{name}")).is_file());
    }
    assert!(!context.root().join("completed_d").exists());
}

#[test]
fn no_cache_does_not_require_a_writable_cache_directory() {
    let context = TestContext::with_files([
        (".karva_cache", "not a directory"),
        ("test.py", "def test_pass(): pass"),
    ]);

    assert_cmd_snapshot!(context.command().arg("--no-cache"), @r###"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_pass
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "###);
}

#[cfg(unix)]
#[test]
fn worker_exit_kills_descendants_holding_controller_streams() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os
import signal


def test_crash_with_child():
    if os.fork() == 0:
        signal.pause()
    os._exit(28)
",
    );

    assert_cmd_snapshot!(context.command(), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           CRASH [TIME] test::test_crash_with_child

    failures:

    test::test_crash_with_child:

    error[worker-crashed]: Worker terminated with exit code 28 while running `test::test_crash_with_child`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 28 in [TIME]
    "###);
}

#[cfg(unix)]
#[test]
fn worker_exit_reports_incomplete_output_from_escaped_descendant() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import time


def test_crash_with_escaped_child():
    ready_read, ready_write = os.pipe()
    if os.fork() == 0:
        os.close(ready_read)
        os.setsid()
        os.write(ready_write, b"1")
        time.sleep(0.2)
        os.write(1, b"late stdout\n")
        os.write(2, b"late stderr\n")
        os._exit(0)
    os.close(ready_write)
    os.read(ready_read, 1)
    os._exit(30)
"#,
    );

    assert_cmd_snapshot!(context.command(), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           CRASH [TIME] test::test_crash_with_escaped_child

    failures:

    test::test_crash_with_escaped_child:

    error[worker-crashed]: Worker terminated with exit code 30 while running `test::test_crash_with_escaped_child`

    Worker stderr:
    [Karva stopped draining worker output after the 50 ms limit; final output and results may be incomplete]

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    WARN worker output drain limit reached; final output and results may be incomplete worker_id=0 limit_ms=50
    ERROR Worker 0 failed with exit code 30 in [TIME]
    "###);
}

#[test]
fn worker_stderr_without_newline_is_captured() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os


def test_crash():
    os.write(2, b"stderr without newline")
    os._exit(29)
"#,
    );

    assert_cmd_snapshot!(context.command(), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           CRASH [TIME] test::test_crash

    failures:

    test::test_crash:

    error[worker-crashed]: Worker terminated with exit code 29 while running `test::test_crash`

    Worker stderr:
    stderr without newline

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    stderr without newline
    ERROR Worker 0 failed with exit code 29 in [TIME]
    "###);
}
