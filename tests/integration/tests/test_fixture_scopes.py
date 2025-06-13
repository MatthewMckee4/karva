from .test_env import TestEnv


def get_source_code(constructor_body: str = "pass") -> list[tuple[str, str]]:
    return [
        ("src/__init__.py", "from .calculator import Calculator"),
        (
            "src/calculator.py",
            f"""
                class Calculator:
                    def __init__(self) -> None:
                        {constructor_body}

                    def add(self, a: int, b: int) -> int:
                        return a + b

                    def subtract(self, a: int, b: int) -> int:
                        return a - b

                    def multiply(self, a: int, b: int) -> int:
                        return a * b

                    def divide(self, a: int, b: int) -> float:
                        return a / b""",
        ),
    ]


def test_function_scopes(test_env: TestEnv) -> None:
    test_env.write_files(
        [
            *get_source_code("print('Calculator initialized')"),
            (
                "tests/conftest.py",
                """
                    from karva import fixture
                    from src import Calculator

                    @fixture
                    def calculator() -> Calculator:
                        return Calculator()""",
            ),
            (
                "tests/test_calculator.py",
                """
                from src import Calculator

                def test_add(calculator: Calculator) -> None:
                    assert calculator.add(1, 2) == 3

                def test_subtract(calculator: Calculator) -> None:
                    assert calculator.subtract(1, 2) == -1""",
            ),
        ],
    )

    assert (
        test_env.run_test()
        == """success: true
exit_code: 0
----- stdout -----
All checks passed!
Calculator initialized
Calculator initialized

----- stderr -----"""
    )

    test_env.cleanup()


def test_module_scopes(test_env: TestEnv) -> None:
    test_env.write_files(
        [
            *get_source_code("print('Calculator initialized')"),
            (
                "tests/conftest.py",
                """
                    from karva import fixture
                    from src import Calculator

                    @fixture(scope="module")
                    def calculator() -> Calculator:
                        return Calculator()""",
            ),
            (
                "tests/test_calculator.py",
                """
                from src import Calculator

                def test_add(calculator: Calculator) -> None:
                    assert calculator.add(1, 2) == 3

                def test_add(calculator: Calculator) -> None:
                    assert calculator.add(1, 2) == 3""",
            ),
        ],
    )

    assert (
        test_env.run_test()
        == """success: true
exit_code: 0
----- stdout -----
All checks passed!
Calculator initialized

----- stderr -----"""
    )

    test_env.cleanup()


def test_package_scopes(test_env: TestEnv) -> None:
    test_env.write_files(
        [
            *get_source_code("print('Calculator initialized')"),
            (
                "tests/conftest.py",
                """
                    from karva import fixture
                    from src import Calculator

                    @fixture(scope="package")
                    def calculator() -> Calculator:
                        return Calculator()""",
            ),
            (
                "tests/test_calculator.py",
                """
                from src import Calculator

                def test_add(calculator: Calculator) -> None:
                    assert calculator.add(1, 2) == 3""",
            ),
            (
                "tests/test_calculator_2.py",
                """
                from src import Calculator

                def test_add(calculator: Calculator) -> None:
                    assert calculator.add(1, 2) == 3""",
            ),
        ],
    )

    assert (
        test_env.run_test()
        == """success: true
exit_code: 0
----- stdout -----
All checks passed!
Calculator initialized

----- stderr -----"""
    )

    test_env.cleanup()


def test_session_scopes(test_env: TestEnv) -> None:
    test_env.write_files(
        [
            *get_source_code("print('Calculator initialized')"),
            (
                "conftest.py",
                """
                    from karva import fixture
                    from src import Calculator

                    @fixture(scope="session")
                    def calculator() -> Calculator:
                        return Calculator()""",
            ),
            (
                "tests/test_calculator.py",
                """
                from src import Calculator

                def test_add(calculator: Calculator) -> None:
                    assert calculator.add(1, 2) == 3""",
            ),
            (
                "tests/test_calculator_2.py",
                """
                from src import Calculator

                def test_add(calculator: Calculator) -> None:
                    assert calculator.add(1, 2) == 3""",
            ),
            (
                "tests/inner/test_calculator.py",
                """
                from src import Calculator

                def test_add(calculator: Calculator) -> None:
                    assert calculator.add(1, 2) == 3""",
            ),
        ],
    )

    output = test_env.run_test()

    assert (
        output
        == """success: true
exit_code: 0
----- stdout -----
All checks passed!
Calculator initialized

----- stderr -----"""
    )

    test_env.cleanup()


def test_mixed_scopes(test_env: TestEnv) -> None:
    test_env.write_files(
        [
            *get_source_code("pass"),
            (
                "conftest.py",
                """
                    from karva import fixture
                    from src import Calculator

                    @fixture(scope="session")
                    def session_calculator() -> Calculator:
                        print("Session calculator initialized")
                        return Calculator()

                    @fixture
                    def function_calculator() -> Calculator:
                        print("Function calculator initialized")
                        return Calculator()""",
            ),
            (
                "tests/test_calculator.py",
                """
                from src import Calculator

                def test_session_fixture(session_calculator: Calculator) -> None:
                    assert session_calculator.add(1, 2) == 3

                def test_function_fixture(function_calculator: Calculator) -> None:
                    assert function_calculator.add(1, 2) == 3

                def test_both_fixtures(session_calculator: Calculator, function_calculator: Calculator) -> None:
                    assert session_calculator.add(1, 2) == 3
                    assert function_calculator.add(1, 2) == 3""",
            ),
        ],
    )

    output = test_env.run_test()

    assert (
        output
        == """success: true
exit_code: 0
----- stdout -----
All checks passed!
Session calculator initialized
Function calculator initialized
Function calculator initialized

----- stderr -----"""
    )

    test_env.cleanup()


def test_fixture_across_files(test_env: TestEnv) -> None:
    test_env.write_files(
        [
            *get_source_code("pass"),
            (
                "conftest.py",
                """
                    from karva import fixture
                    from src import Calculator

                    @fixture(scope="package")
                    def package_calculator() -> Calculator:
                        print("Package calculator initialized")
                        return Calculator()""",
            ),
            (
                "tests/test_calculator.py",
                """
                from src import Calculator

                def test_package_fixture(package_calculator: Calculator) -> None:
                    assert package_calculator.add(1, 2) == 3""",
            ),
            (
                "tests/another_test.py",
                """
                from src import Calculator

                def test_same_package_fixture(package_calculator: Calculator) -> None:
                    assert package_calculator.add(1, 2) == 3""",
            ),
        ],
    )

    assert (
        test_env.run_test()
        == """success: true
exit_code: 0
----- stdout -----
All checks passed!
Package calculator initialized

----- stderr -----"""
    )
    test_env.cleanup()


def test_fixture_initialization_order(test_env: TestEnv) -> None:
    test_env.write_files(
        [
            *get_source_code("pass"),
            (
                "conftest.py",
                """
                    from karva import fixture
                    from src import Calculator

                    @fixture(scope="session")
                    def session_calculator() -> Calculator:
                        print("Session calculator initialized")
                        return Calculator()

                    @fixture(scope="module")
                    def module_calculator() -> Calculator:
                        print("Module calculator initialized")
                        return Calculator()

                    @fixture(scope="package")
                    def package_calculator() -> Calculator:
                        print("Package calculator initialized")
                        return Calculator()

                    @fixture
                    def function_calculator() -> Calculator:
                        print("Function calculator initialized")
                        return Calculator()""",
            ),
            (
                "tests/test_calculator.py",
                """
                from src import Calculator

                def test_all_scopes(
                    session_calculator: Calculator,
                    module_calculator: Calculator,
                    package_calculator: Calculator,
                    function_calculator: Calculator,
                ) -> None:
                    assert session_calculator.add(1, 2) == 3
                    assert module_calculator.add(1, 2) == 3
                    assert package_calculator.add(1, 2) == 3
                    assert function_calculator.add(1, 2) == 3""",
            ),
        ],
    )

    assert (
        test_env.run_test()
        == """success: true
exit_code: 0
----- stdout -----
All checks passed!
Session calculator initialized
Package calculator initialized
Module calculator initialized
Function calculator initialized

----- stderr -----"""
    )
    test_env.cleanup()
