from io import TextIOWrapper  # noqa: D100, INP001
from pathlib import Path
from typing import Protocol

RESOURCES_PATH = Path(__file__).parent


class AddTestFunction(Protocol):
    """A function that adds a body to a test function."""

    def __call__(self, f: TextIOWrapper, num_asserts: int) -> None:
        """Add a body to a test function."""


def add_true_assertions(f: TextIOWrapper, num_asserts: int) -> None:
    """Add true assertions to a test function."""
    for _ in range(num_asserts):
        f.write("    assert True\n")


def add_math(f: TextIOWrapper, num_asserts: int) -> None:
    """Add complex math to a test function."""
    for _ in range(num_asserts):
        f.write("    x = 2\n")
        f.write("    y = 3\n")
        f.write("    assert x ** y == 8\n")


def add_string_concatenation(f: TextIOWrapper, num_asserts: int) -> None:
    """Add string concatenation to a test function."""
    for _ in range(num_asserts):
        f.write("    assert 'hello' + 'world' == 'helloworld'\n")


def add_large_summation(f: TextIOWrapper, num_asserts: int) -> None:
    """Add a large summation to a test function."""
    number = 10_000
    for _ in range(num_asserts):
        f.write(
            f"    assert sum(range({number})) == {number * (number - 1) // 2}\n",
        )


def add_large_list_comprehension(f: TextIOWrapper, num_asserts: int) -> None:
    """Add a large list comprehension to a test function."""
    number = 10_000
    for _ in range(num_asserts):
        f.write(
            f"    assert [i for i in range({number})] == list(range({number}))\n",
        )


def generate_test_file(
    path: Path,
    *,
    num_tests: int = 10000,
    num_asserts_per_function: int = 1,
    add_test_function: AddTestFunction = add_true_assertions,
) -> None:
    """Generate a test file with the specified number of individual test functions."""
    with path.open("w") as f:
        for i in range(num_tests):
            f.write(f"def test_{i}():\n")
            add_test_function(f, num_asserts_per_function)
            f.write("\n")


NUM_TESTS = 10000
NUM_ASSERT_PER_FUNCTION = 2

OUTPUT_RESOURCES = [
    (RESOURCES_PATH / "test_true_assertions.py", NUM_TESTS, 1, add_true_assertions),
    (RESOURCES_PATH / "test_math.py", NUM_TESTS, 1, add_math),
    (
        RESOURCES_PATH / "test_string_concatenation.py",
        NUM_TESTS,
        1,
        add_string_concatenation,
    ),
    (RESOURCES_PATH / "test_large_summation.py", NUM_TESTS, 1, add_large_summation),
    (
        RESOURCES_PATH / "test_large_list_comprehension.py",
        NUM_TESTS,
        1,
        add_large_list_comprehension,
    ),
]


def main() -> None:
    """Generate the test files."""
    for path, num_tests, num_bodies, add_test_function in OUTPUT_RESOURCES:
        generate_test_file(
            path,
            num_tests=num_tests,
            num_asserts_per_function=num_bodies,
            add_test_function=add_test_function,
        )


if __name__ == "__main__":
    main()
