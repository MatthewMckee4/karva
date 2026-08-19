use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;
use regex::Regex;
use rstest::rstest;

use crate::common::TestContext;

const RECOVERED_TEST: &str = r#"
import os
from pathlib import Path


def test_recovered():
    Path("completed-by").write_text(os.environ["KARVA_WORKER_ID"])
"#;

#[rstest]
fn worker_exit_before_authentication_recovers_selection(
    #[values("before-connect", "before-hello", "during-hello")] failure: &str,
) {
    let context = TestContext::with_file("test.py", RECOVERED_TEST);

    insta::allow_duplicates! {
        assert_cmd_snapshot!(handshake_failure_command(&context, failure), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_recovered

        diagnostics:

        error[worker-crashed]: Worker 0 terminated with exit code 38 during startup before controller authentication

        Karva preserved 0 completed test results from this assignment and is retrying 1 unstarted test selection in a replacement worker.

        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ERROR Worker 0 failed with exit code 38 in [TIME]
        ");
    }
    assert_eq!(context.read_file("completed-by"), "1");
}

#[test]
fn worker_disconnect_during_selection_recovers_selection() {
    let context = TestContext::with_file("test.py", RECOVERED_TEST);

    assert_cmd_snapshot!(handshake_failure_command(&context, "during-selection"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_recovered

    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 38 with no active test checkpoint

    Karva preserved 0 completed test results from this assignment and is retrying 1 unstarted test selection in a replacement worker.

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 38 in [TIME]
    ");
    assert_eq!(context.read_file("completed-by"), "1");
    assert_eq!(context.read_file("selection-received"), "1");
}

#[test]
fn partial_handshake_held_by_escaped_descendant_does_not_block_recovery() {
    let context = TestContext::with_file("test.py", RECOVERED_TEST);
    let output = run_with_escaped_connection(&context, "during-hello-escaped");
    terminate_escaped_descendant(&context);

    assert_snapshot!(snapshot_output(&output), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_recovered

    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 38 during startup before controller authentication

    Karva preserved 0 completed test results from this assignment and is retrying 1 unstarted test selection in a replacement worker.

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 38 in [TIME]
    ");
    assert_eq!(context.read_file("completed-by"), "1");
}

#[test]
fn continuous_events_from_escaped_descendant_do_not_extend_crash_drain() {
    let context = TestContext::with_file("test.py", RECOVERED_TEST);
    let output = run_with_escaped_connection(&context, "continuous-events-escaped");
    terminate_escaped_descendant(&context);

    assert_snapshot!(snapshot_output(&output), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 38 after its 50 ms controller event drain limit expired without a decoded active test checkpoint

    Karva preserved 0 completed test results from this assignment and omitted 1 remaining test selection from recovery because their execution state could not be determined safely. They are absent from the results; rerun them only if repeat execution is safe.

    Worker stderr:
    [Karva stopped draining worker output after the 50 ms limit; final output and results may be incomplete]

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped, [COUNT] slow

    ----- stderr -----
    WARN worker output drain limit reached; final output and results may be incomplete worker_id=0 limit_ms=50
    ERROR Worker 0 failed with exit code 38 in [TIME]
    ");
}

fn run_with_escaped_connection(context: &TestContext, failure: &str) -> Output {
    let mut karva = handshake_failure_command(context, failure)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn Karva");

    // Receipt: the existing handshake suite completed within 0.72 seconds in
    // 20 local runs. Five seconds leaves more than six times that headroom.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if karva.try_wait().expect("poll Karva").is_some() {
            return karva.wait_with_output().expect("collect Karva output");
        }
        if Instant::now() >= deadline {
            karva.kill().expect("kill stalled Karva");
            let descendant_cleanup = kill_escaped_descendant(context);
            let output = karva.wait_with_output().expect("collect Karva output");
            panic!(
                "Karva stalled on an escaped worker connection (descendant cleanup: {descendant_cleanup:?})\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn snapshot_output(output: &Output) -> String {
    let output = format!(
        "success: {}\nexit_code: {}\n----- stdout -----\n{}\n----- stderr -----\n{}",
        output.status.success(),
        output
            .status
            .code()
            .map_or_else(|| "none".to_string(), |code| code.to_string()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    Regex::new(r", \d+ slow")
        .expect("valid slow-count regex")
        .replace_all(&output, ", [COUNT] slow")
        .into_owned()
}

fn terminate_escaped_descendant(context: &TestContext) {
    assert_eq!(kill_escaped_descendant(context), Some(true));
}

fn kill_escaped_descendant(context: &TestContext) -> Option<bool> {
    std::fs::read_to_string(context.root().join("descendant_pid"))
        .ok()
        .map(|pid| {
            Command::new("kill")
                .args(["-KILL", pid.trim()])
                .status()
                .is_ok_and(|status| status.success())
        })
}

fn handshake_failure_command(context: &TestContext, failure: &str) -> Command {
    let worker = context.root().join(".venv/bin/karva-worker");
    std::fs::create_dir_all(worker.parent().expect("worker binary parent"))
        .expect("create project virtualenv bin directory");
    std::fs::write(&worker, FAKE_WORKER).expect("write fake worker binary");
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o755))
        .expect("make fake worker executable");

    let path = std::env::join_paths(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|directory| !directory.join("karva-worker").is_file())
            .chain([PathBuf::from("/usr/bin"), PathBuf::from("/bin")]),
    )
    .expect("worker-free executable path");
    let mut command = context.command();
    command
        .env("PATH", path)
        .env("KARVA_HANDSHAKE_FAILURE", failure);
    command
}

const FAKE_WORKER: &str = r#"#!/bin/sh
worker_id=
controller_address=
run_id=
previous=
for argument in "$@"; do
    case "$previous" in
        --worker-id) worker_id=$argument ;;
        --controller-address) controller_address=$argument ;;
        --run-id) run_id=$argument ;;
    esac
    previous=$argument
done

if [ "$worker_id" != "0" ]; then
    exec "$VIRTUAL_ENV/bin/karva-worker" "$@"
fi

if [ "$KARVA_HANDSHAKE_FAILURE" = "before-connect" ]; then
    exit 38
fi

"$VIRTUAL_ENV/bin/python" - \
    "$controller_address" \
    "$run_id" \
    "$worker_id" \
    "$KARVA_HANDSHAKE_FAILURE" <<'PY'
import json
import os
from pathlib import Path
import signal
import socket
import sys
import time


controller_address, run_id, worker_id, failure = sys.argv[1:]
if controller_address.startswith("unix:"):
    connection = socket.socket(socket.AF_UNIX)
    connection.connect(controller_address.removeprefix("unix:"))
else:
    host, port = controller_address.removeprefix("tcp:").rsplit(":", 1)
    connection = socket.create_connection((host, int(port)))

if failure == "during-hello":
    connection.sendall(b'{"Hello":')
elif failure == "during-hello-escaped":
    connection.sendall(b'{"Hello":')
    if os.fork() == 0:
        os.setsid()
        signal.alarm(30)
        Path("descendant_pid").write_text(str(os.getpid()))
        os.close(0)
        os.close(1)
        os.close(2)
        signal.pause()
    while not Path("descendant_pid").exists():
        time.sleep(0.001)
elif failure == "continuous-events-escaped":
    hello = {"Hello": {"run_id": run_id, "worker_id": int(worker_id)}}
    connection.sendall(json.dumps(hello).encode() + b"\n")
    # Receipt: this four-case suite took at most 0.72s across 20 local runs.
    connection.settimeout(5)
    selection = b""
    while not selection.endswith(b"\n"):
        chunk = connection.recv(4096)
        if not chunk:
            raise RuntimeError("controller closed before sending the worker selection")
        selection += chunk
    if os.fork() == 0:
        os.setsid()
        signal.alarm(30)
        Path("descendant_pid").write_text(str(os.getpid()))
        os.close(0)
        os.close(1)
        os.close(2)
        connection.settimeout(None)
        try:
            while True:
                connection.sendall(b'{"Event":"TestSlow"}\n')
        except OSError:
            signal.pause()
    while not Path("descendant_pid").exists():
        time.sleep(0.001)
elif failure == "during-selection":
    hello = {"Hello": {"run_id": run_id, "worker_id": int(worker_id)}}
    connection.sendall(json.dumps(hello).encode() + b"\n")
    # Receipt: this four-case suite took at most 0.72s across 20 local runs.
    connection.settimeout(5)
    if not connection.recv(1):
        raise RuntimeError("controller closed before sending the worker selection")
    with open("selection-received", "w", encoding="utf-8") as marker:
        marker.write("1")
connection.close()
PY
exit 38
"#;
