use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[rstest]
fn tags_are_available_throughout_each_case(
    #[values("karva", "pytest")] framework: &str,
    #[values("--no-parallel", "--num-workers=2")] execution: &str,
) {
    let (tags, autouse, tagged_case) = if framework == "pytest" {
        (
            "pytest.mark",
            "autouse",
            "pytest.param(True, marks=[pytest.mark.skip_invariant])",
        )
    } else {
        (
            "karva.tags",
            "auto_use",
            "karva.param(True, tags=[karva.tags.skip_invariant])",
        )
    };
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
import karva
import pytest

pytestmark = pytest.mark.inherited
history = []
active = None

@{framework}.fixture({autouse}=True)
def enforce_invariant(test_info):
    global active
    active = not test_info.has_tag("skip_invariant")
    yield
    assert active == (not test_info.has_tag("skip_invariant"))

@{framework}.fixture
async def metadata(test_info):
    assert test_info.has_tag("inherited")
    for previous, names in history:
        assert previous.tags == names
        assert previous is not test_info
    history.append((test_info, test_info.tags))
    yield test_info
    assert test_info.has_tag("inherited")

@{tags}.custom(42, mode="fast")
@{tags}.custom
@{tags}.parametrize("bypass", [
    {tagged_case},
    False,
])
async def test_tagged(bypass, metadata, test_info: karva.TestInfo):
    assert metadata is test_info
    assert isinstance(test_info, karva.TestInfo)
    assert isinstance(test_info.tags, frozenset)
    assert test_info.tags == frozenset(["inherited", "custom", "parametrize"] + (["skip_invariant"] if bypass else []))
    assert test_info.has_tag("skip_invariant") == bypass
    assert not test_info.has_tag("absent")
    assert active == (not bypass)
    with karva.raises(AttributeError):
        test_info.tags = frozenset()
    with karva.raises(AttributeError):
        test_info.tags.add("injected")

@{tags}.parametrize("case", [1, 2])
def test_other(case, metadata, test_info):
    assert metadata is test_info
    assert test_info.tags == frozenset(["inherited", "parametrize"])
    assert active
"#,
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command().args([execution, "--status-level=none"]), @"
        success: true
        exit_code: 0
        ----- stdout -----
        ────────────
             Summary [TIME] 4 tests run: 4 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn untagged_test_has_empty_metadata() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.fixture
def indirect(test_info):
    return test_info

def test_empty(test_info: karva.TestInfo, indirect):
    assert indirect is test_info
    assert test_info.tags == frozenset()
    assert not test_info.has_tag("custom")
"#,
    );
    assert_cmd_snapshot!(context.command().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn retries_create_fresh_metadata() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import karva

previous = []
active = None

@karva.fixture
async def metadata(test_info):
    global active
    active = test_info
    assert all(test_info is not old for old in previous)
    previous.append(test_info)
    yield
    assert test_info.tags == frozenset(["custom", "parametrize"])

@karva.tags.custom
@karva.tags.parametrize("case", [1, 2])
def test_retry(case, metadata, test_info):
    assert active is test_info
    assert test_info.has_tag("custom")
    assert os.environ["KARVA_ATTEMPT"] == "2"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--retry=1",
        "--status-level=none",
        "--final-status-level=skip",
    ]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 2 passed (2 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry(case=1, metadata=None, test_info)
       FLAKY 2/2 [TIME] test::test_retry(case=2, metadata=None, test_info)

    ----- stderr -----
    ");
}

#[rstest]
fn broader_scopes_reject_test_info(#[values("module", "package", "session")] scope: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
import karva

@karva.fixture(scope="{scope}")
def shared(test_info):
    raise AssertionError("fixture must not execute")

def test_scope(shared):
    raise AssertionError("test must not execute")
"#,
        ),
    );
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(
        r"[^\s]+[/\\]karva[/\\]_builtins\.py",
        "<builtins>/karva/_builtins.py",
    );
    settings.add_filter(&format!("with `{scope}` scope"), "with `<scope>` scope");
    let _guard = settings.bind_to_scope();
    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test::test_scope

        failures:

        test::test_scope:

        error[fixture-scope-mismatch]: Fixture `shared` with `<scope>` scope cannot depend on fixture `test_info` with `function` scope
         --> test.py:5:5
          |
        5 | def shared(test_info):
          |     ^^^^^^
        info: Fixture `test_info` has `function` scope
           --> <builtins>/karva/_builtins.py:534:5
            |
        534 | def current_test_info(*, _tags: Sequence[str] | None = None) -> TestInfo:
            |     ^^^^^^^^^^^^^^^^^

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn user_override_can_depend_on_builtin_test_info() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.fixture
def test_info(test_info):
    return sorted(test_info.tags)

@karva.tags.custom
def test_override(test_info):
    assert test_info == ["custom"]
"#,
    );
    assert_cmd_snapshot!(context.command().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn side_effect_fixture_receives_case_tags() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

seen = None

@karva.fixture
def inspect_tags(test_info):
    global seen
    seen = test_info.tags

@karva.tags.custom
@karva.tags.use_fixtures("inspect_tags")
def test_used():
    assert seen == frozenset(["custom", "use_fixtures"])
"#,
    );
    assert_cmd_snapshot!(context.command().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn builtin_pytest_marker_names_are_normalized() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.fixture
def inspect_tags(test_info):
    assert test_info.tags == frozenset(["skip", "expect_fail", "use_fixtures"])

@pytest.mark.skipif(False, reason="enabled")
@pytest.mark.xfail(False, reason="enabled")
@pytest.mark.usefixtures("inspect_tags")
def test_markers(test_info):
    assert test_info.has_tag("expect_fail")
    assert not test_info.has_tag("xfail")
"#,
    );
    assert_cmd_snapshot!(context.command().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}
