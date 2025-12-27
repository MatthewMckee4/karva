use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_use_fixtures_single_fixture() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

arr = []

@karva.fixture
def setup_fixture():
    arr.append(1)

@karva.tags.use_fixtures("setup_fixture")
def test_with_use_fixture():
    assert arr == [1]
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_with_use_fixture ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_use_fixtures_multiple_fixtures() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

arr = []

@karva.fixture
def fixture1():
    arr.append(1)

@karva.fixture
def fixture2():
    arr.append(2)

@karva.tags.use_fixtures("fixture1", "fixture2")
def test_with_multiple_use_fixtures():
    assert arr == [1, 2]
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_with_multiple_use_fixtures ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_use_fixtures_combined_with_parameter_fixtures() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.fixture
def setup_fixture():
    return "setup_value"

@karva.fixture
def param_fixture():
    return "param_value"

@karva.tags.use_fixtures("setup_fixture")
def test_combined_fixtures(param_fixture):
    # Both setup_fixture (from use_fixtures) and param_fixture (from parameters) should be resolved
    assert True
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_combined_fixtures(param_fixture=param_value) ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_use_fixtures_with_parametrize() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

arr = []

@karva.fixture
def setup_fixture():
    arr.append(1)

@karva.tags.use_fixtures("setup_fixture")
@karva.tags.parametrize("value", [1, 2, 3])
def test_use_fixtures_with_parametrize(value):
    assert len(arr) == value
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_use_fixtures_with_parametrize(value=1) ... ok
    test test::test_use_fixtures_with_parametrize(value=2) ... ok
    test test::test_use_fixtures_with_parametrize(value=3) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_use_fixtures_multiple_decorators() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

arr = []

@karva.fixture
def fixture1():
    arr.append(1)

@karva.fixture
def fixture2():
    arr.append(2)

@karva.tags.use_fixtures("fixture1")
@karva.tags.use_fixtures("fixture2")
def test_multiple_use_fixtures_decorators():
    assert 1 in arr
    assert 2 in arr
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_multiple_use_fixtures_decorators ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_use_fixtures_fixture_not_found_but_not_used() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.tags.use_fixtures("nonexistent_fixture")
def test_missing_fixture():
    assert True
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_missing_fixture ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_use_fixtures_generator_fixture() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

arr = []

@karva.fixture
def generator_fixture():
    arr.append(1)
    yield 1

@karva.tags.use_fixtures("generator_fixture")
def test_use_fixtures_with_generator():
    assert arr == [1]
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_use_fixtures_with_generator ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_use_fixtures_session_scope() {
    let test_context = TestContext::with_files([(
        "test.py",
        r#"
import karva

arr = []

@karva.fixture(scope='session')
def session_fixture():
    arr.append(1)

@karva.tags.use_fixtures("session_fixture")
def test_session_1():
    assert arr == [1]

@karva.tags.use_fixtures("session_fixture")
def test_session_2():
    assert arr == [1]
"#,
    )]);

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_session_1 ... ok
    test test::test_session_2 ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_use_fixtures_mixed_with_normal_fixtures() {
    let test_context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import karva

@karva.fixture
def shared_fixture():
    return "shared_value"

@karva.fixture
def use_fixture_only():
    return "use_only_value"
"#,
        ),
        (
            "test.py",
            r#"
import karva

@karva.tags.use_fixtures("use_fixture_only")
def test_mixed_fixtures(shared_fixture):
    assert shared_fixture == "shared_value"
"#,
        ),
    ]);

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_mixed_fixtures(shared_fixture=shared_value) ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_pytest_mark_usefixtures_single_fixture() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import pytest

arr = []

@pytest.fixture
def setup_fixture():
    arr.append(1)

@pytest.mark.usefixtures("setup_fixture")
def test_with_pytest_use_fixture():
    assert arr == [1]
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_with_pytest_use_fixture ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_pytest_mark_usefixtures_multiple_fixtures() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import pytest

arr = []

@pytest.fixture
def fixture1():
    arr.append(1)

@pytest.fixture
def fixture2():
    arr.append(2)

@pytest.mark.usefixtures("fixture1", "fixture2")
def test_with_multiple_pytest_use_fixtures():
    assert arr == [1, 2]
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_with_multiple_pytest_use_fixtures ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_pytest_mark_usefixtures_with_parametrize() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import pytest

arr = []

@pytest.fixture
def setup_fixture():
    arr.append(1)

@pytest.mark.usefixtures("setup_fixture")
@pytest.mark.parametrize("value", [1, 2, 3])
def test_pytest_use_fixtures_with_parametrize(value):
    assert value > 0
    # Fixtures are called before each run
    assert len(arr) == value
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_pytest_use_fixtures_with_parametrize(value=1) ... ok
    test test::test_pytest_use_fixtures_with_parametrize(value=2) ... ok
    test test::test_pytest_use_fixtures_with_parametrize(value=3) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_pytest_mark_usefixtures_session_scope() {
    let test_context = TestContext::with_files([(
        "test.py",
        r#"
import pytest

arr = []

@pytest.fixture(scope='session')
def session_fixture():
    arr.append(1)

@pytest.mark.usefixtures("session_fixture")
def test_pytest_session_1():
    assert arr == [1]

@pytest.mark.usefixtures("session_fixture")
def test_pytest_session_2():
    assert arr == [1]
"#,
    )]);

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_pytest_session_1 ... ok
    test test::test_pytest_session_2 ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
