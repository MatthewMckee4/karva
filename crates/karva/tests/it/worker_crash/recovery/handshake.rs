use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use insta_cmd::assert_cmd_snapshot;
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
import socket
import sys


controller_address, run_id, worker_id, failure = sys.argv[1:]
if controller_address.startswith("unix:"):
    connection = socket.socket(socket.AF_UNIX)
    connection.connect(controller_address.removeprefix("unix:"))
else:
    host, port = controller_address.removeprefix("tcp:").rsplit(":", 1)
    connection = socket.create_connection((host, int(port)))

if failure == "during-hello":
    connection.sendall(b'{"Hello":')
elif failure == "during-selection":
    hello = {"Hello": {"run_id": run_id, "worker_id": int(worker_id)}}
    connection.sendall(json.dumps(hello).encode() + b"\n")
    # Receipt: this four-case suite took at most 0.72s across 20 local runs.
    connection.settimeout(5)
    connection.recv(1)
connection.close()
PY
exit 38
"#;
