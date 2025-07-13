ITERATIONS := "1"
NUM_TESTS := "10000"
N := "1000"

# Build the project
build:
    uv venv
    cargo build

# Run tests
test:
    uv run --no-project maturin build
    cargo test

# Build documentation
docs:
    uv run --no-project mkdocs build

# Serve documentation locally
docs-serve:
    uv sync --group docs --no-install-project
    uv run --no-project mkdocs serve

# Format code
format:
    cargo +nightly fmt
    cargo sort

# Run benchmarks
benchmark: build
    cd scripts/benchmark && uv sync --all-extras --no-install-project && uv pip install -e ../../ && uv run main.py --iterations {{ITERATIONS}} --num-tests {{NUM_TESTS}} --run-test
