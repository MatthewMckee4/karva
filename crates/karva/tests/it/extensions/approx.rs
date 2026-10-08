use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_approx_default_relative_tolerance() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import pytest

def test_comparison():
    actual = 1.0000001
    expected = 1
    assert (actual == karva.approx(expected)) is True
    assert (actual == pytest.approx(expected)) is True
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_outside_default_relative_tolerance() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import pytest

def test_comparison():
    actual = 1.0001
    expected = 1
    assert (actual == karva.approx(expected)) is False
    assert (actual == pytest.approx(expected)) is False
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_explicit_relative_tolerance() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import pytest

def test_comparison():
    actual = 1.0001
    expected = 1
    assert (actual == karva.approx(expected, rel=1e-3)) is True
    assert (actual == pytest.approx(expected, rel=1e-3)) is True
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_explicit_absolute_tolerance_disables_default_relative() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import pytest

def test_comparison():
    actual = 1.00000001
    expected = 1
    assert (actual == karva.approx(expected, abs=1e-12)) is False
    assert (actual == pytest.approx(expected, abs=1e-12)) is False
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_default_absolute_tolerance_near_zero() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import pytest

def test_comparison():
    actual = 1e-13
    expected = 0
    assert (actual == karva.approx(expected)) is True
    assert (actual == pytest.approx(expected)) is True
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_complex_values() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import pytest

def test_comparison():
    actual = complex(1.0000001, 2)
    expected = complex(1, 2)
    assert (actual == karva.approx(expected)) is True
    assert (actual == pytest.approx(expected)) is True
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_decimal_values() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import pytest
from decimal import Decimal

def test_comparison():
    actual = Decimal("1.0000001")
    expected = Decimal("1")
    assert (actual == karva.approx(expected)) is True
    assert (actual == pytest.approx(expected)) is True
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_sequence_values() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import pytest

def test_comparison():
    actual = [0.1 + 0.2, 0.6]
    expected = [0.3, 0.6]
    assert (actual == karva.approx(expected)) is True
    assert (actual == pytest.approx(expected)) is True
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_sequence_length_mismatch() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import pytest

def test_comparison():
    actual = [0.3]
    expected = [0.3, 0.6]
    assert (actual == karva.approx(expected)) is False
    assert (actual == pytest.approx(expected)) is False
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_mapping_values() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import pytest

def test_comparison():
    actual = {"x": 0.1 + 0.2}
    expected = {"x": 0.3}
    assert (actual == karva.approx(expected)) is True
    assert (actual == pytest.approx(expected)) is True
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_mapping_key_mismatch() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import pytest

def test_comparison():
    actual = {"y": 0.3}
    expected = {"x": 0.3}
    assert (actual == karva.approx(expected)) is False
    assert (actual == pytest.approx(expected)) is False
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_nan_is_unequal_by_default() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import pytest
import math

def test_comparison():
    actual = math.nan
    expected = math.nan
    assert (actual == karva.approx(expected)) is False
    assert (actual == pytest.approx(expected)) is False
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_nan_ok_accepts_nan() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import pytest
import math

def test_comparison():
    actual = math.nan
    expected = math.nan
    assert (actual == karva.approx(expected, nan_ok=True)) is True
    assert (actual == pytest.approx(expected, nan_ok=True)) is True
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_same_infinities() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import pytest
import math

def test_comparison():
    actual = math.inf
    expected = math.inf
    assert (actual == karva.approx(expected)) is True
    assert (actual == pytest.approx(expected)) is True
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_opposite_infinities() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import pytest
import math

def test_comparison():
    actual = -math.inf
    expected = math.inf
    assert (actual == karva.approx(expected)) is False
    assert (actual == pytest.approx(expected)) is False
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_equality_works_on_either_side() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_comparison():
    assert karva.approx(0.3) == 0.1 + 0.2
    assert 0.1 + 0.2 == karva.approx(0.3)
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_comparison
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_representation_includes_tolerance() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_representation():
    assert repr(karva.approx(0.3)) == "0.3 ± 3.0e-07"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_representation
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

