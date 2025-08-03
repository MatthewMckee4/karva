N := "1000"

# Build the project
build:
    uv run --no-project maturin build

# Run tests
test *args:
    @rm -f target/wheels/*.whl
    uv run --no-project --with maturin maturin build
    cargo test {{args}}

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
benchmark iterations: build
    cd scripts/benchmark && uv sync --all-extras --no-install-project && uv pip install -e ../../ && uv run main.py --iterations {{iterations}} --num-tests 10000 --run-test
