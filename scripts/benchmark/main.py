import argparse
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

import matplotlib.pyplot as plt
import numpy as np


@dataclass
class Benchmark:
    name: str
    command: str

    def run_benchmark(self, iterations: int = 5) -> float:
        print(f"Running {self.name} benchmark {iterations} times")
        return run_benchmark(self.command, iterations)


def generate_test_file(
    *,
    num_tests: int = 10000,
    num_asserts: int = 1,
    import_name: str,
    fail: bool = False,
) -> Path:
    """Generate a test file with the specified number of individual test functions."""
    test_file = Path(f"test_{import_name}_many_assertions.py")

    fixture_lines = [
        f"from {import_name} import fixture",
        "@fixture",
        "def value_one() -> int:",
        "    return 1",
    ]

    with test_file.open("w") as f:
        f.write("\n".join(fixture_lines) + "\n")

        def test_function(i: int) -> list[str]:
            lines = [
                f"def test_{i}(value_one):",
            ]

            lines.extend(
                [
                    f"    assert value_one {'==' if not fail else '!='} 1",
                ]
                * num_asserts,
            )

            return lines

        for i in range(num_tests):
            f.write("\n".join(test_function(i)) + "\n")

    return test_file


def run_benchmark(command: str, iterations: int = 5) -> float:
    """Run a benchmark command multiple times and return mean and standard deviation."""
    times: list[float] = []
    for _ in range(iterations + 1):
        start = time.time()
        result = subprocess.run(command, shell=True, capture_output=True, check=False)
        print(result.stdout.decode("utf-8"))
        print(result.stderr.decode("utf-8"))
        time_taken = time.time() - start
        print(f"Time taken: {time_taken:.4f}s")
        times.append(time_taken)
    return float(np.mean(times[1:]))


def create_benchmark_graph(
    benchmarks: list[Benchmark],
    *,
    iterations: int = 5,
    num_tests: int = 10000,
) -> None:
    """Create and save a benchmark comparison graph."""
    plt.style.use("dark_background")

    labels = [benchmark.name for benchmark in benchmarks]
    means = [benchmark.run_benchmark(iterations) for benchmark in benchmarks]

    y_pos = np.arange(len(labels))

    fig, ax = plt.subplots(figsize=(8, 2))
    fig.patch.set_facecolor("black")
    ax.set_facecolor("black")

    ax.spines["top"].set_visible(False)
    ax.spines["right"].set_visible(False)
    ax.spines["left"].set_visible(False)
    ax.spines["bottom"].set_visible(True)
    ax.tick_params(
        axis="both",  # Changed from "x" to "both" to affect both axes
        which="both",
        bottom=True,
        top=False,
        labelbottom=True,
        colors="grey",
    )
    ax.xaxis.set_ticks_position("bottom")
    ax.xaxis.set_label_position("bottom")
    ax.spines["bottom"].set_color("grey")

    max_time = np.ceil(max(means))
    linspace = np.linspace(0, max_time, 5)
    ax.set_xticks(linspace)
    ax.set_xticklabels(
        [f"{x:.2f}s" for x in linspace],
        color="grey",
    )

    bars = ax.barh(y_pos, means, color=["#45744a", "#45744a"], height=0.5)

    ax.set_yticks(y_pos)
    ax.set_yticklabels(labels, fontsize=16, color="grey")

    for bar in bars:
        width = bar.get_width()
        y = bar.get_y() + bar.get_height() / 2.0
        ax.text(
            width + max(means) * 0.01,
            y,
            f"{width:.2f}s",
            ha="left",
            va="center",
            color="grey",
            fontsize=10,
        )

    plt.title(
        f"Running on a file with {num_tests:,} tests",
        fontsize=18,
        pad=20,
        color="grey",
        y=-0.6,
    )

    for path in [
        "../../docs/assets/benchmark_results.svg",
    ]:
        plt.savefig(
            path,
            dpi=600,
            bbox_inches="tight",
            transparent=True,
        )

    plt.close()


def main() -> None:
    """Run the complete benchmark process."""
    parser = argparse.ArgumentParser(description="Run benchmark tests")
    parser.add_argument(
        "--num-tests",
        type=int,
        default=10000,
        help="Number of tests to generate (default: 10000)",
    )
    parser.add_argument(
        "--iterations",
        type=int,
        default=1,
        help="Number of benchmark iterations to run (default: 1)",
    )
    parser.add_argument(
        "--num-asserts",
        type=int,
        default=1,
        help="Number of assertions to generate (default: 1)",
    )
    parser.add_argument(
        "--keep-test-file",
        action="store_true",
        default=False,
        help="Keep the test file after running the benchmark",
    )
    parser.add_argument(
        "--run-test",
        action="store_true",
        default=False,
        help="Run the benchmark with flamegraph",
    )
    parser.add_argument(
        "--fail",
        action="store_true",
        default=False,
        help="Use failing tests",
    )
    args = parser.parse_args()

    print(
        f"Generating test files with {args.num_tests} tests and {args.num_asserts} assertions",
    )

    karva_test_file = generate_test_file(
        num_tests=args.num_tests,
        num_asserts=args.num_asserts,
        import_name="karva",
        fail=args.fail,
    )

    pytest_test_file = generate_test_file(
        num_tests=args.num_tests,
        num_asserts=args.num_asserts,
        import_name="pytest",
        fail=args.fail,
    )

    benchmarks: list[Benchmark] = [
        Benchmark(
            name="pytest",
            command=f"uv run pytest {pytest_test_file}",
        ),
        Benchmark(
            name="karva",
            command=f"uv run karva test {karva_test_file}",
        ),
    ]
    if args.run_test:
        create_benchmark_graph(
            benchmarks,
            iterations=args.iterations,
            num_tests=args.num_tests,
        )


if __name__ == "__main__":
    main()
