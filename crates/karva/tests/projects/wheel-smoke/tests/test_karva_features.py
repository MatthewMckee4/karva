from __future__ import annotations

import os

import karva
from karva_wheel_smoke.calculator import Calculator, legacy_label, normalize_name


@karva.fixture
def calculator() -> Calculator:
    """Provide a calculator with a fixed offset."""
    return Calculator(offset=2)


@karva.tags.parametrize(
    "left,right,expected",
    [
        (1, 2, 5),
        karva.param(3, 4, 9),
    ],
)
def test_parametrized_addition(
    calculator: Calculator,
    left: int,
    right: int,
    expected: int,
) -> None:
    """Verify parametrized fixture inputs."""
    assert calculator.add(left, right) == expected


def test_tmp_path_and_output_capture(tmp_path, capsys) -> None:
    """Verify temporary paths and output capture."""
    message_file = tmp_path / "message.txt"
    message_file.write_text("hello from wheel smoke", encoding="utf-8")

    print(message_file.read_text(encoding="utf-8"))

    captured = capsys.readouterr()
    assert captured.out == "hello from wheel smoke\n"
    assert captured.err == ""


def test_warning_capture(recwarn) -> None:
    """Verify warning capture."""
    assert legacy_label("  Example  ") == "legacy:example"

    warning = recwarn.pop(DeprecationWarning)
    assert str(warning.message) == "legacy_label is deprecated"


def test_monkeypatch(monkeypatch) -> None:
    """Verify environment patching."""
    monkeypatch.setenv("KARVA_WHEEL_SMOKE", "installed")

    assert os.environ["KARVA_WHEEL_SMOKE"] == "installed"


def test_raises_helper() -> None:
    """Verify exception assertions."""
    with karva.raises(ValueError, match="empty"):
        normalize_name("  ")


@karva.tags.expect_fail(reason="exercise expected failure handling")
def test_expected_failure_tag() -> None:
    """Verify expected-failure handling."""
    raise AssertionError("expected failure")


@karva.tags.skip(reason="exercise skip handling")
def test_skip_tag() -> None:
    """Verify skip handling."""
    raise AssertionError("skip tag did not skip this test")
