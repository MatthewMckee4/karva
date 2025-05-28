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
        return run_benchmark(self.command, iterations)


def generate_test_file(num_tests: int = 10000, num_asserts: int = 1) -> Path:
    """Generate a test file with the specified number of individual test functions."""
    test_file = Path("test_many_assertions.py")
    with test_file.open("w") as f:
        f.write("def test_0():\n    assert True\n\n")
        for i in range(1, num_tests):
            f.write(f"def test_{i}():\n")
            for _ in range(num_asserts):
                f.write("    assert True\n")
            f.write("\n")
    return test_file


def run_benchmark(command: str, iterations: int = 5) -> float:
    """Run a benchmark command multiple times and return mean and standard deviation."""
    times: list[float] = []
    for _ in range(iterations):
        start = time.time()
        subprocess.run(command, shell=True, capture_output=True, check=False)  # noqa: S602
        end = time.time()
        times.append(end - start)
    return float(np.mean(times))


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
    ax.tick_params(axis="x", which="both", bottom=True, top=False, labelbottom=True)
    ax.xaxis.set_ticks_position("bottom")
    ax.xaxis.set_label_position("bottom")

    max_time = np.ceil(max(means))
    linspace = np.linspace(0, max_time, 5)
    ax.set_xticks(linspace)
    ax.set_xticklabels(
        [f"{x:.2f}s" for x in linspace],
        color="white",
    )

    bars = ax.barh(y_pos, means, color=["#4ECDC4", "#4ECDC4"], height=0.5)

    ax.set_yticks(y_pos)
    ax.set_yticklabels(labels, fontsize=16)

    for bar in bars:
        width = bar.get_width()
        y = bar.get_y() + bar.get_height() / 2.0
        ax.text(
            width + max(means) * 0.01,
            y,
            f"{width:.2f}s",
            ha="left",
            va="center",
            color="white",
            fontsize=10,
        )

    plt.title(
        f"Running on a file with {num_tests:,} tests",
        fontsize=18,
        pad=20,
        color="white",
        y=-0.6,
    )

    for path in [
        "../../assets/benchmark_results.svg",
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
    args = parser.parse_args()

    test_file = generate_test_file(args.num_tests)

    benchmarks: list[Benchmark] = [
        Benchmark(
            name="pytest",
            command=f"pytest {test_file}",
        ),
        Benchmark(
            name="karva",
            command=f"../../target/debug/karva test {test_file}",
        ),
    ]

    create_benchmark_graph(
        benchmarks,
        iterations=args.iterations,
        num_tests=args.num_tests,
    )

    test_file.unlink()


if __name__ == "__main__":
    main()
