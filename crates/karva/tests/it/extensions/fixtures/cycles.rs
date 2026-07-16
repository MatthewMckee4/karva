use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_direct_fixture_cycle() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
from pathlib import Path

@karva.fixture
def value(value):
    Path("fixture-ran").touch()

def test_cycle(value):
    Path("test-ran").touch()
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_cycle

    diagnostics:

    error[fixture-cycle]: Fixture dependency cycle detected
     --> test.py:6:5
      |
    6 | def value(value):
      |     ^^^^^
      |
    info: value -> value

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("fixture-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_async_generator_fixture_cycle() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
from pathlib import Path

@karva.fixture
async def database(client):
    Path("database-ran").touch()
    return client

@karva.fixture
def client(database):
    Path("client-ran").touch()
    yield database

def test_cycle(database):
    Path("test-ran").touch()
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_cycle

    diagnostics:

    error[fixture-cycle]: Fixture dependency cycle detected
     --> test.py:6:11
      |
    6 | async def database(client):
      |           ^^^^^^^^
      |
    info: Fixture `client` requires `database`
      --> test.py:11:5
       |
    11 | def client(database):
       |     ^^^^^^
       |
    info: database -> client -> database

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("database-ran").exists());
    assert!(!context.root().join("client-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_fixture_cycle_across_module_and_conftest() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import karva
from pathlib import Path

@karva.fixture
def database(client):
    Path("database-ran").touch()
    return client
"#,
        ),
        (
            "test.py",
            r#"
import karva
from pathlib import Path

@karva.fixture
def client(database):
    Path("client-ran").touch()
    return database

def test_cycle(database):
    Path("test-ran").touch()
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_cycle

    diagnostics:

    error[fixture-cycle]: Fixture dependency cycle detected
     --> conftest.py:6:5
      |
    6 | def database(client):
      |     ^^^^^^^^
      |
    info: Fixture `client` requires `database`
     --> test.py:6:5
      |
    6 | def client(database):
      |     ^^^^^^
      |
    info: database -> client -> database

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("database-ran").exists());
    assert!(!context.root().join("client-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_cycle_excludes_noncyclic_prefix() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
from pathlib import Path

@karva.fixture
def requested(first):
    Path("fixture-ran").touch()

@karva.fixture
def first(second):
    Path("fixture-ran").touch()

@karva.fixture
def second(third):
    Path("fixture-ran").touch()

@karva.fixture
def third(first):
    Path("fixture-ran").touch()

def test_cycle(requested):
    Path("test-ran").touch()
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_cycle

    diagnostics:

    error[fixture-cycle]: Fixture dependency cycle detected
      --> test.py:10:5
       |
    10 | def first(second):
       |     ^^^^^
       |
    info: Fixture `second` requires `third`
      --> test.py:14:5
       |
    14 | def second(third):
       |     ^^^^^^
       |
    info: Fixture `third` requires `first`
      --> test.py:18:5
       |
    18 | def third(first):
       |     ^^^^^
       |
    info: first -> second -> third -> first

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("fixture-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_function_autouse_fixture_cycle() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
from pathlib import Path

@karva.fixture(auto_use=True)
def first(second):
    Path("fixture-ran").touch()

@karva.fixture
def second(first):
    Path("fixture-ran").touch()

def test_cycle():
    Path("test-ran").touch()
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_cycle

    diagnostics:

    error[fixture-cycle]: Fixture dependency cycle detected
     --> test.py:6:5
      |
    6 | def first(second):
      |     ^^^^^
      |
    info: Fixture `second` requires `first`
      --> test.py:10:5
       |
    10 | def second(first):
       |     ^^^^^^
       |
    info: first -> second -> first

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("fixture-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_module_autouse_fixture_cycle() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
from pathlib import Path

@karva.fixture(scope="module", auto_use=True)
def first(second):
    Path("first-ran").touch()

@karva.fixture(scope="module")
def second(first):
    Path("second-ran").touch()

def test_cycle_one():
    Path("test-one-ran").touch()

def test_cycle_two():
    Path("test-two-ran").touch()
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] test::test_cycle_one
            FAIL [TIME] test::test_cycle_two

    diagnostics:

    error[fixture-cycle]: Fixture dependency cycle detected
     --> test.py:6:5
      |
    6 | def first(second):
      |     ^^^^^
      |
    info: Fixture `second` requires `first`
      --> test.py:10:5
       |
    10 | def second(first):
       |     ^^^^^^
       |
    info: first -> second -> first

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("first-ran").exists());
    assert!(!context.root().join("second-ran").exists());
    assert!(!context.root().join("test-one-ran").exists());
    assert!(!context.root().join("test-two-ran").exists());
}

#[test]
fn test_package_autouse_fixture_cycle() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import karva
from pathlib import Path

@karva.fixture(scope="package", auto_use=True)
def first(second):
    Path("fixture-ran").touch()

@karva.fixture(scope="package")
def second(first):
    Path("fixture-ran").touch()
"#,
        ),
        (
            "test.py",
            r#"
from pathlib import Path

def test_cycle():
    Path("test-ran").touch()
"#,
        ),
        (
            "nested/test_nested.py",
            r#"
from pathlib import Path

def test_nested_cycle():
    Path("nested-test-ran").touch()
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] test::test_cycle
            FAIL [TIME] nested.test_nested::test_nested_cycle

    diagnostics:

    error[fixture-cycle]: Fixture dependency cycle detected
     --> conftest.py:6:5
      |
    6 | def first(second):
      |     ^^^^^
      |
    info: Fixture `second` requires `first`
      --> conftest.py:10:5
       |
    10 | def second(first):
       |     ^^^^^^
       |
    info: first -> second -> first

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("fixture-ran").exists());
    assert!(!context.root().join("test-ran").exists());
    assert!(!context.root().join("nested-test-ran").exists());
}

#[test]
fn test_session_autouse_fixture_cycle() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import karva
from pathlib import Path

@karva.fixture(scope="session", auto_use=True)
def first(second):
    Path("fixture-ran").touch()

@karva.fixture(scope="session")
def second(first):
    Path("fixture-ran").touch()
"#,
        ),
        (
            "test.py",
            r#"
from pathlib import Path

def test_cycle():
    Path("test-ran").touch()
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_cycle

    diagnostics:

    error[fixture-cycle]: Fixture dependency cycle detected
     --> conftest.py:6:5
      |
    6 | def first(second):
      |     ^^^^^
      |
    info: Fixture `second` requires `first`
      --> conftest.py:10:5
       |
    10 | def second(first):
       |     ^^^^^^
       |
    info: first -> second -> first

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("fixture-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_use_fixtures_cycle() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
from pathlib import Path

@karva.fixture
def first(second):
    Path("fixture-ran").touch()

@karva.fixture
def second(first):
    Path("fixture-ran").touch()

@karva.tags.use_fixtures("first")
def test_cycle():
    Path("test-ran").touch()
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_cycle

    diagnostics:

    error[fixture-cycle]: Fixture dependency cycle detected
     --> test.py:6:5
      |
    6 | def first(second):
      |     ^^^^^
      |
    info: Fixture `second` requires `first`
      --> test.py:10:5
       |
    10 | def second(first):
       |     ^^^^^^
       |
    info: first -> second -> first

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("fixture-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_diamond_fixture_graph() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

calls = 0

@karva.fixture
def shared():
    global calls
    calls += 1
    return calls

@karva.fixture
def left(shared):
    return shared

@karva.fixture
def right(shared):
    return shared

def test_diamond(left, right):
    assert left == right == 1
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_diamond(left=1, right=1)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}
