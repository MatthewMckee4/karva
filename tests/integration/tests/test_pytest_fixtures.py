# from .test_env import TestEnv


# def get_source_code(constructor_body: str = "pass") -> list[tuple[str, str]]:
#     return [
#         ("src/__init__.py", "from .calculator import Calculator"),
#         (
#             "src/calculator.py",
#             f"""
#                 class Calculator:
#                     def __init__(self) -> None:
#                         {constructor_body}

#                     def add(self, a: int, b: int) -> int:
#                         return a + b

#                     def subtract(self, a: int, b: int) -> int:
#                         return a - b

#                     def multiply(self, a: int, b: int) -> int:
#                         return a * b

#                     def divide(self, a: int, b: int) -> float:
#                         return a / b""",
#         ),
#     ]


# def test_function_scopes(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("print('Calculator initialized')"),
#             (
#                 "tests/conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture
#                     def calculator() -> Calculator:
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_add(calculator: Calculator) -> None:
#                     assert calculator.add(1, 2) == 3

#                 def test_subtract(calculator: Calculator) -> None:
#                     assert calculator.subtract(1, 2) == -1""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Calculator initialized
# Calculator initialized

# ----- stderr -----"""
#     )

#     test_env.cleanup()


# def test_module_scopes(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("print('Calculator initialized')"),
#             (
#                 "tests/conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(scope="module")
#                     def calculator() -> Calculator:
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_add(calculator: Calculator) -> None:
#                     assert calculator.add(1, 2) == 3

#                 def test_add(calculator: Calculator) -> None:
#                     assert calculator.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Calculator initialized

# ----- stderr -----"""
#     )

#     test_env.cleanup()


# def test_package_scopes(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("print('Calculator initialized')"),
#             (
#                 "tests/conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(scope="package")
#                     def calculator() -> Calculator:
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_add(calculator: Calculator) -> None:
#                     assert calculator.add(1, 2) == 3""",
#             ),
#             (
#                 "tests/test_calculator_2.py",
#                 """
#                 from src import Calculator

#                 def test_add(calculator: Calculator) -> None:
#                     assert calculator.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Calculator initialized

# ----- stderr -----"""
#     )

#     test_env.cleanup()


# def test_session_scopes(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("print('Calculator initialized')"),
#             (
#                 "conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(scope="session")
#                     def calculator() -> Calculator:
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_add(calculator: Calculator) -> None:
#                     assert calculator.add(1, 2) == 3""",
#             ),
#             (
#                 "tests/test_calculator_2.py",
#                 """
#                 from src import Calculator

#                 def test_add(calculator: Calculator) -> None:
#                     assert calculator.add(1, 2) == 3""",
#             ),
#             (
#                 "tests/inner/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_add(calculator: Calculator) -> None:
#                     assert calculator.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     output = test_env.run_test()

#     assert (
#         output
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Calculator initialized

# ----- stderr -----"""
#     )

#     test_env.cleanup()


# def test_mixed_scopes(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(scope="session")
#                     def session_calculator() -> Calculator:
#                         print("Session calculator initialized")
#                         return Calculator()

#                     @fixture
#                     def function_calculator() -> Calculator:
#                         print("Function calculator initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_session_fixture(session_calculator: Calculator) -> None:
#                     assert session_calculator.add(1, 2) == 3

#                 def test_function_fixture(function_calculator: Calculator) -> None:
#                     assert function_calculator.add(1, 2) == 3

#                 def test_both_fixtures(session_calculator: Calculator, function_calculator: Calculator) -> None:
#                     assert session_calculator.add(1, 2) == 3
#                     assert function_calculator.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     output = test_env.run_test()

#     assert (
#         output
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Session calculator initialized
# Function calculator initialized
# Function calculator initialized

# ----- stderr -----"""
#     )

#     test_env.cleanup()


# def test_fixture_across_files(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(scope="package")
#                     def package_calculator() -> Calculator:
#                         print("Package calculator initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_package_fixture(package_calculator: Calculator) -> None:
#                     assert package_calculator.add(1, 2) == 3""",
#             ),
#             (
#                 "tests/another_test.py",
#                 """
#                 from src import Calculator

#                 def test_same_package_fixture(package_calculator: Calculator) -> None:
#                     assert package_calculator.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Package calculator initialized

# ----- stderr -----"""
#     )
#     test_env.cleanup()


# def test_fixture_initialization_order(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(scope="session")
#                     def session_calculator() -> Calculator:
#                         print("Session calculator initialized")
#                         return Calculator()

#                     @fixture(scope="module")
#                     def module_calculator() -> Calculator:
#                         print("Module calculator initialized")
#                         return Calculator()

#                     @fixture(scope="package")
#                     def package_calculator() -> Calculator:
#                         print("Package calculator initialized")
#                         return Calculator()

#                     @fixture
#                     def function_calculator() -> Calculator:
#                         print("Function calculator initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_all_scopes(
#                     session_calculator: Calculator,
#                     module_calculator: Calculator,
#                     package_calculator: Calculator,
#                     function_calculator: Calculator,
#                 ) -> None:
#                     assert session_calculator.add(1, 2) == 3
#                     assert module_calculator.add(1, 2) == 3
#                     assert package_calculator.add(1, 2) == 3
#                     assert function_calculator.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Session calculator initialized
# Package calculator initialized
# Module calculator initialized
# Function calculator initialized

# ----- stderr -----"""
#     )
#     test_env.cleanup()


# def test_named_fixtures(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("print('Named calculator initialized')"),
#             (
#                 "tests/conftest.py",
#                 """
#                 from pytest import fixture
#                 from src import Calculator

#                 @fixture(name="named_calculator")
#                 def calculator() -> Calculator:
#                     return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_named_fixture(named_calculator: Calculator) -> None:
#                     assert named_calculator.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Named calculator initialized

# ----- stderr -----"""
#     )
#     test_env.cleanup()


# def test_nested_package_scopes(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from pytest import fixture
#                 from src import Calculator

#                 @fixture
#                 def calculator() -> Calculator:
#                     print("Calculator initialized")
#                     return Calculator()

#                 def test_add(calculator: Calculator) -> None:
#                     assert calculator.add(1, 2) == 3""",
#             ),
#             (
#                 "tests/inner/conftest.py",
#                 """
#                 from pytest import fixture
#                 from src import Calculator

#                 @fixture(scope="package")
#                 def package_calculator() -> Calculator:
#                     print("Package calculator initialized")
#                     return Calculator()""",
#             ),
#             (
#                 "tests/inner/sub/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_add(package_calculator: Calculator) -> None:
#                     assert package_calculator.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Calculator initialized
# Package calculator initialized

# ----- stderr -----"""
#     )

#     test_env.cleanup()


# def test_independent_fixtures(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "tests/conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture
#                     def calculator_a() -> Calculator:
#                         print("Calculator A initialized")
#                         return Calculator()

#                     @fixture
#                     def calculator_b() -> Calculator:
#                         print("Calculator B initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_a(calculator_a: Calculator) -> None:
#                     assert calculator_a.add(1, 2) == 3

#                 def test_b(calculator_b: Calculator) -> None:
#                     assert calculator_b.multiply(2, 3) == 6""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Calculator A initialized
# Calculator B initialized

# ----- stderr -----"""
#     )
#     test_env.cleanup()


# def test_multiple_files_independent_fixtures(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "tests/conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(scope="module")
#                     def multiply_calculator() -> Calculator:
#                         print("Multiply calculator initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_add.py",
#                 """
#                 from src import Calculator

#                 def test_add_1(multiply_calculator: Calculator) -> None:
#                     assert multiply_calculator.add(1, 2) == 3

#                 def test_add_2(multiply_calculator: Calculator) -> None:
#                     assert multiply_calculator.add(3, 4) == 7""",
#             ),
#             (
#                 "tests/test_multiply.py",
#                 """
#                 from src import Calculator

#                 def test_multiply_1(multiply_calculator: Calculator) -> None:
#                     assert multiply_calculator.multiply(2, 3) == 6

#                 def test_multiply_2(multiply_calculator: Calculator) -> None:
#                     assert multiply_calculator.multiply(4, 5) == 20""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Multiply calculator initialized
# Multiply calculator initialized

# ----- stderr -----"""
#     )
#     test_env.cleanup()


# def test_basic_error_handling(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "tests/conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture
#                     def failing_calculator() -> Calculator:
#                         raise RuntimeError("Fixture initialization failed")

#                     @fixture
#                     def working_calculator() -> Calculator:
#                         print("Working calculator initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_working(working_calculator: Calculator) -> None:
#                     assert working_calculator.add(1, 2) == 3

#                 def test_failing(failing_calculator: Calculator) -> None:
#                     assert failing_calculator.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: false
# exit_code: 1
# ----- stdout -----
# error[fixture-not-found] in <temp_dir>/tests/test_calculator.py
#  | Fixture failing_calculator not found
# -------------
# Passed tests: 1
# Error tests: 1
# Working calculator initialized

# ----- stderr -----
# ERROR Failed to call fixture failing_calculator: RuntimeError: Fixture initialization failed"""
#     )
#     test_env.cleanup()


# def test_different_scopes_independent(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(scope="session")
#                     def session_calculator() -> Calculator:
#                         print("Session calculator initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(scope="package")
#                     def package_calculator() -> Calculator:
#                         print("Package calculator initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/inner/conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(scope="module")
#                     def module_calculator() -> Calculator:
#                         print("Module calculator initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/inner/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_session(session_calculator: Calculator) -> None:
#                     assert session_calculator.add(1, 2) == 3

#                 def test_package(package_calculator: Calculator) -> None:
#                     assert package_calculator.subtract(5, 3) == 2

#                 def test_module(module_calculator: Calculator) -> None:
#                     assert module_calculator.multiply(2, 3) == 6""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Session calculator initialized
# Package calculator initialized
# Module calculator initialized

# ----- stderr -----"""
#     )
#     test_env.cleanup()


# def test_invalid_scope_value(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "tests/conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(scope="invalid_scope")
#                     def calculator() -> Calculator:
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_calc(calculator: Calculator) -> None:
#                     assert calculator.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: false
# exit_code: 1
# ----- stdout -----
# error[invalid-fixture] in <temp_dir>/tests/conftest.py
#  | Invalid fixture scope: invalid_scope
# error[fixture-not-found] in <temp_dir>/tests/test_calculator.py
#  | Fixture calculator not found
# -------------
# Error tests: 1

# ----- stderr -----"""
#     )
#     test_env.cleanup()


# def test_empty_conftest(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "conftest.py",
#                 """
#                     # Empty conftest file""",
#             ),
#             (
#                 "tests/conftest.py",
#                 """
#                     # Another empty conftest file""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_no_fixtures() -> None:
#                     calculator = Calculator()
#                     assert calculator.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!

# ----- stderr -----"""
#     )
#     test_env.cleanup()


# def test_invalid_fixture_name(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "tests/conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(name="123invalid")
#                     def calculator() -> Calculator:
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_calc(calculator: Calculator) -> None:
#                     assert calculator.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: false
# exit_code: 1
# ----- stdout -----
# error[fixture-not-found] in <temp_dir>/tests/test_calculator.py
#  | Fixture calculator not found
# -------------
# Error tests: 1

# ----- stderr -----"""
#     )
#     test_env.cleanup()


# def test_multiple_conftest_same_dir(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "tests/conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture
#                     def calculator_1() -> Calculator:
#                         print("Calculator 1 initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/conftest_more.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture
#                     def calculator_2() -> Calculator:
#                         print("Calculator 2 initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_calc(calculator_1: Calculator, calculator_2: Calculator) -> None:
#                     assert calculator_1.add(1, 2) == 3
#                     assert calculator_2.multiply(2, 3) == 6""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: false
# exit_code: 1
# ----- stdout -----
# error[fixture-not-found] in <temp_dir>/tests/test_calculator.py
#  | Fixture calculator_2 not found
# -------------
# Error tests: 1
# Calculator 1 initialized

# ----- stderr -----"""
#     )
#     test_env.cleanup()


# def test_very_deep_directory_structure(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(scope="session")
#                     def root_calc() -> Calculator:
#                         print("Root calculator initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/level1/level2/level3/level4/level5/conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture(scope="package")
#                     def deep_calc() -> Calculator:
#                         print("Deep calculator initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/level1/level2/level3/level4/level5/test_deep.py",
#                 """
#                 from src import Calculator

#                 def test_deep(root_calc: Calculator, deep_calc: Calculator) -> None:
#                     assert root_calc.add(1, 2) == 3
#                     assert deep_calc.multiply(2, 3) == 6""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Root calculator initialized
# Deep calculator initialized

# ----- stderr -----"""
#     )
#     test_env.cleanup()


# def test_fixture_in_init_file(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "tests/__init__.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture
#                     def init_calculator() -> Calculator:
#                         print("Init calculator initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_init_fixture(init_calculator: Calculator) -> None:
#                     assert init_calculator.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: false
# exit_code: 1
# ----- stdout -----
# error[fixture-not-found] in <temp_dir>/tests/test_calculator.py
#  | Fixture init_calculator not found
# -------------
# Error tests: 1

# ----- stderr -----"""
#     )
#     test_env.cleanup()


# def test_same_fixture_name_different_types(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("pass"),
#             (
#                 "tests/math/conftest.py",
#                 """
#                     from pytest import fixture
#                     from src import Calculator

#                     @fixture
#                     def value() -> Calculator:
#                         print("Calculator value initialized")
#                         return Calculator()""",
#             ),
#             (
#                 "tests/math/test_math.py",
#                 """
#                 from src import Calculator

#                 def test_math_value(value: Calculator) -> None:
#                     assert value.add(1, 2) == 3""",
#             ),
#             (
#                 "tests/string/conftest.py",
#                 """
#                     from pytest import fixture

#                     @fixture
#                     def value() -> str:
#                         print("Calculator value initialized")
#                         return "test"
#                     """,
#             ),
#             (
#                 "tests/string/test_string.py",
#                 """
#                 def test_string_value(value: str) -> None:
#                     assert value == "test"
#                     """,
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Calculator value initialized
# Calculator value initialized

# ----- stderr -----"""
#     )
#     test_env.cleanup()


# def test_fixture_dependencies(test_env: TestEnv) -> None:
#     test_env.write_files(
#         [
#             *get_source_code("print('Calculator initialized')"),
#             (
#                 "tests/conftest.py",
#                 """
#                 from pytest import fixture
#                 from src import Calculator

#                 @fixture
#                 def calculator() -> Calculator:
#                     print("Calculator fixture initialized")
#                     return Calculator()

#                 @fixture
#                 def calculator_with_value(calculator: Calculator) -> Calculator:
#                     print("Calculator with value fixture initialized")
#                     calculator.add(5, 5)
#                     return calculator""",
#             ),
#             (
#                 "tests/test_calculator.py",
#                 """
#                 from src import Calculator

#                 def test_calculator_with_value(calculator_with_value: Calculator) -> None:
#                     assert calculator_with_value.add(1, 2) == 3

#                 def test_calculator_dependency(calculator: Calculator, calculator_with_value: Calculator) -> None:
#                     assert calculator.add(1, 2) == 3
#                     assert calculator_with_value.add(1, 2) == 3""",
#             ),
#         ],
#     )

#     assert (
#         test_env.run_test()
#         == """success: true
# exit_code: 0
# ----- stdout -----
# All checks passed!
# Calculator fixture initialized
# Calculator initialized
# Calculator with value fixture initialized
# Calculator fixture initialized
# Calculator initialized
# Calculator with value fixture initialized

# ----- stderr -----"""
#     )
#     test_env.cleanup()
