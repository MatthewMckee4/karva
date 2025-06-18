# noqa: INP001
from dataclasses import dataclass
from io import TextIOWrapper
from pathlib import Path
from typing import Protocol

RESOURCES_PATH = Path(__file__).parent

TAB = "    "


class AddTestFunction(Protocol):
    """A function that adds a body to a test function."""

    def __call__(self, f: TextIOWrapper, test_index: int, num_asserts: int) -> None:
        """Add a body to a test function."""


def add_true_assertions(f: TextIOWrapper, test_index: int, num_asserts: int) -> None:
    """Add true assertions to a test function."""
    f.writelines(f"def test_{test_index}():\n" for _ in range(num_asserts))
    f.writelines(f"{TAB}assert True\n" for _ in range(num_asserts))


def add_math(f: TextIOWrapper, test_index: int, num_asserts: int) -> None:
    """Add complex math to a test function."""
    f.writelines(f"def test_{test_index}():\n" for _ in range(num_asserts))
    for _ in range(num_asserts):
        f.write(f"{TAB}x = 2\n")
        f.write(f"{TAB}y = 3\n")
        f.write(f"{TAB}assert x ** y == 8\n")


def add_string_concatenation(
    f: TextIOWrapper,
    test_index: int,
    num_asserts: int,
) -> None:
    """Add string concatenation to a test function."""
    f.writelines(f"def test_{test_index}():\n" for _ in range(num_asserts))
    f.writelines(
        f"{TAB}assert 'hello' + 'world' == 'helloworld'\n" for _ in range(num_asserts)
    )


def add_large_summation(f: TextIOWrapper, test_index: int, num_asserts: int) -> None:
    """Add a large summation to a test function."""
    f.writelines(f"def test_{test_index}():\n" for _ in range(num_asserts))
    number = 10_000
    f.writelines(
        f"{TAB}assert sum(range({number})) == {number * (number - 1) // 2}\n"
        for _ in range(num_asserts)
    )


def add_large_list_comprehension(
    f: TextIOWrapper,
    test_index: int,
    num_asserts: int,
) -> None:
    """Add a large list comprehension to a test function."""
    number = 10_000
    f.write(f"def test_{test_index}():\n")
    f.writelines(
        f"{TAB}assert [i for i in range({number})] == list(range({number}))\n"
        for _ in range(num_asserts)
    )


def _fixture_name(i: int) -> str:
    return f"fixture_{i}"


def add_fixture(
    f: TextIOWrapper,
    test_index: int,
    num_asserts: int,
) -> None:
    """Add a fixture to a test function."""
    fixture_arguments = ", ".join(_fixture_name(i) for i in range(10))
    f.write(f"def test_{test_index}({fixture_arguments}):\n")
    f.writelines(f"{TAB}assert True\n" for _ in range(num_asserts))


NUM_TESTS = 100
NUM_ASSERT_PER_FUNCTION = 5


@dataclass
class Benchmark:
    path: Path

    num_tests: int
    num_asserts_per_function: int

    add_test_function: AddTestFunction
    before_tests: str = ""


OUTPUT_RESOURCES = [
    Benchmark(
        path=RESOURCES_PATH / "test_true_assertions.py",
        num_tests=NUM_TESTS,
        num_asserts_per_function=NUM_ASSERT_PER_FUNCTION,
        add_test_function=add_true_assertions,
    ),
    Benchmark(
        path=RESOURCES_PATH / "test_math.py",
        num_tests=NUM_TESTS,
        num_asserts_per_function=NUM_ASSERT_PER_FUNCTION,
        add_test_function=add_math,
    ),
    Benchmark(
        path=RESOURCES_PATH / "test_string_concatenation.py",
        num_tests=NUM_TESTS,
        num_asserts_per_function=NUM_ASSERT_PER_FUNCTION,
        add_test_function=add_string_concatenation,
    ),
    Benchmark(
        path=RESOURCES_PATH / "test_large_summation.py",
        num_tests=NUM_TESTS,
        num_asserts_per_function=NUM_ASSERT_PER_FUNCTION,
        add_test_function=add_large_summation,
    ),
    Benchmark(
        path=RESOURCES_PATH / "test_large_list_comprehension.py",
        num_tests=NUM_TESTS,
        num_asserts_per_function=NUM_ASSERT_PER_FUNCTION,
        add_test_function=add_large_list_comprehension,
    ),
    Benchmark(
        path=RESOURCES_PATH / "test_fixtures.py",
        num_tests=NUM_TESTS,
        num_asserts_per_function=NUM_ASSERT_PER_FUNCTION,
        add_test_function=add_fixture,
        before_tests="""
class FixtureFunctionMarker:
    def __init__(self, scope, name = None):
        self.scope = scope
        self.name = name

    def __call__(self, function):
        return FixtureFunctionDefinition(
            function=function,
            fixture_function_marker=self,
        )

class FixtureFunctionDefinition:
    def __init__(
        self,
        *,
        function,
        fixture_function_marker,
    ):
        self.name = fixture_function_marker.name or function.__name__
        self.__name__ = self.name
        self._fixture_function_marker = fixture_function_marker
        self._fixture_function = function

    def __get__(
        self,
        instance = None,
        owner = None,
    ):
        return self

    def __call__(self, *args, **kwds):
        return self._fixture_function(*args, **kwds)

def fixture(
    fixture_function = None,
    *,
    scope = 'function',
    name = None,
):
    fixture_marker = FixtureFunctionMarker(
        scope=scope,
        name=name,
    )
    if fixture_function:
        return fixture_marker(fixture_function)
    return fixture_marker
"""
        + "\n".join(f"@fixture\ndef {_fixture_name(i)}(): pass" for i in range(10))
        + "\n",
    ),
]


def generate_test_file(benchmark: Benchmark) -> None:
    """Generate a test file with the specified number of individual test functions."""
    with benchmark.path.open("w") as f:
        f.write(benchmark.before_tests)
        f.write("\n")
        for i in range(benchmark.num_tests):
            benchmark.add_test_function(f, i, benchmark.num_asserts_per_function)
            f.write("\n")


def main() -> None:
    """Generate the test files."""
    for benchmark in OUTPUT_RESOURCES:
        generate_test_file(benchmark)


def clear_files() -> None:
    """Remove contents of the files."""
    for benchmark in OUTPUT_RESOURCES:
        benchmark.path.write_text("")


if __name__ == "__main__":
    main()
