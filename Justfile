N := "1000"

# Build the project
build:
    uv run --no-project maturin build

# Run tests
test *args:
    @rm -f target/wheels/*.whl
    uv run --no-project --with maturin --isolated maturin build
    cargo test {{args}}

# Build documentation
docs:
    uv venv --clear
    uv sync --group docs --no-install-project
    uv run --no-project mkdocs build

# Serve documentation locally
docs-serve:
    uv venv --clear
    uv sync --group docs --no-install-project
    uv run --no-project mkdocs serve
