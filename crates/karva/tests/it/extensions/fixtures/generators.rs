use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_generator() {
    let test_context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.fixture
def fixture_generator():
    yield 1

def test_fixture_generator(fixture_generator):
    assert fixture_generator == 1
",
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_fixture_generator(fixture_generator=1) ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_generator_with_second_fixture(#[values("karva", "pytest")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{framework}.fixture
def first_fixture():
    pass

@{framework}.fixture
def fixture_generator(first_fixture):
    yield 1

def test_fixture_generator(fixture_generator):
    assert fixture_generator == 1
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_fixture_generator(fixture_generator=1) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}
