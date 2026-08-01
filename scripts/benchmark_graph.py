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
    plt.rcParams.update(
        {
            "font.family": "DejaVu Sans",
            "svg.fonttype": "none",
        }
    )

    labels = ["pytest", "pytest-xdist", "karva"]
    means = [92.2, 60.5, 2.6]
    foreground = "#ebf4dd"
    muted = "#b9c9b5"

    y_pos = np.arange(len(labels))

    fig, ax = plt.subplots(figsize=(7.2, 2.6))
    fig.patch.set_alpha(0)
    ax.set_facecolor("none")

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
        colors=muted,
    )
    ax.xaxis.set_ticks_position("bottom")
    ax.xaxis.set_label_position("bottom")
    ax.spines["bottom"].set_color(muted)

    max_time = np.ceil(max(means))
    linspace = np.linspace(0, max_time, 5)
    ax.set_xticks(linspace)
    ax.set_xticklabels(
        [f"{x:.2f}s" for x in linspace],
        color=muted,
        fontsize=9,
    )

    bars = ax.barh(
        y_pos,
        means,
        color=["#5a7863", "#90ab8b", foreground],
        height=0.46,
    )

    ax.set_yticks(y_pos)
    ax.set_yticklabels(labels, fontsize=12, color=foreground, fontweight=700)

    for bar in bars:
        width = bar.get_width()
        y = bar.get_y() + bar.get_height() / 2.0
        ax.text(
            width + max(means) * 0.01,
            y,
            f"{width:.2f}s",
            ha="left",
            va="center",
            color=foreground,
            fontsize=9,
        )

    fig.text(
        0.99,
        0.015,
        "Workload: ~250,000 tests · Machine: 14 cores",
        ha="right",
        color=muted,
        fontsize=8,
    )

    fig.subplots_adjust(bottom=0.22, left=0.2, right=0.9, top=0.96)

    for path in [
        ROOT / "docs/assets/benchmark_results.svg",
    ]:
        plt.savefig(
            path,
            dpi=600,
            bbox_inches="tight",
            transparent=True,
        )
        svg = path.read_text().replace(
            "font-family: 'DejaVu Sans'",
            "font-family: Manrope, system-ui, sans-serif",
        )
        path.write_text("\n".join(line.rstrip() for line in svg.splitlines()) + "\n")

    plt.close()


if __name__ == "__main__":
    main()
