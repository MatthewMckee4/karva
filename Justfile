N := "1000"

# Build the project
build:
    uv run --no-project maturin build

# Run tests
test *args:
    @rm -f target/wheels/*.whl
    uv venv --clear
    uv pip install maturin pytest
    uv run --no-project maturin build
    uv run cargo test {{args}}

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

pytest-benchmark iterations: build
    cd scripts/benchmark && uv sync --all-extras --no-install-project && uv pip install -e ../../ && uv run main.py --iterations {{iterations}} --num-tests 10000 --run-test

# Run benchmarks
benchmark:
    cargo codspeed build --features codspeed -p karva_benchmark
    cargo codspeed run
