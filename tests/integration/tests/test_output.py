from .test_env import TestEnv


def test_std_out_not_redirected(test_env: TestEnv) -> None:
    """Test that stdout is properly captured and displayed."""
    test_env.write_files(
        [
            ("test_std_out_redirected.py", "def test_std_out_redirected(): print('Hello, world!')"),
        ],
    )

    assert (
        test_env.run_test()
        == """success: true
exit_code: 0
----- stdout -----
Passed tests: 1
All checks passed!
Hello, world!

----- stderr -----"""
    )


def test_std_out_redirected(test_env: TestEnv) -> None:
    """Test that stdout is properly captured and displayed."""
    test_env.write_files(
        [
            ("test_std_out_redirected.py", "def test_std_out_redirected(): print('Hello, world!')"),
        ],
    )

    assert (
        test_env.run_test(())
        == """success: true
exit_code: 0
----- stdout -----
Passed tests: 1
All checks passed!

----- stderr -----"""
    )
