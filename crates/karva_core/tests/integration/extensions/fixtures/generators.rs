use karva_core::TestResultStats;
use karva_test::TestContext;
use rstest::rstest;

use crate::common::TestRunnerExt;

#[test]
fn test_fixture_generator() {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
        r"
import karva

@karva.fixture
def fixture_generator():
    yield 1

def test_fixture_generator(fixture_generator):
    assert fixture_generator == 1
",
    );

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    expected_stats.add_passed();

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}

#[rstest]
fn test_fixture_generator_with_second_fixture(#[values("karva", "pytest")] framework: &str) {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
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

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    expected_stats.add_passed();

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}
