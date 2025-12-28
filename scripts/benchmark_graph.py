# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "matplotlib",
#     "numpy",
# ]
# ///
#
# We hardcode these values from running the commands ourselves.

from pathlib import Path

import matplotlib.pyplot as plt
import numpy as np

ROOT = Path(__file__).parent.parent


def main() -> None:
    """Create and save a benchmark comparison graph."""
    plt.style.use("dark_background")

    labels = ["pytest", "pytest-xdist (20 cores)", "karva (20 cores)"]
    means = [51.96, 21.34, 2.55]

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
        "Running pydantic tests",
        fontsize=18,
        pad=20,
        color="grey",
        y=-0.6,
    )

    for path in [
        ROOT / "docs/assets/benchmark_results.svg",
    ]:
        plt.savefig(
            path,
            dpi=600,
            bbox_inches="tight",
            transparent=True,
        )

    plt.close()


if __name__ == "__main__":
    main()
