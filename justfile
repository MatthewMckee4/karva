# https://just.systems

test *args:
    uvx maturin build
    @if command -v cargo-nextest > /dev/null 2>&1; then \
        cargo nextest run {{args}}; \
    else \
        cargo test {{args}}; \
    fi

benchmark *args:
    uv run --no-project --with maturin maturin build
    cargo run --release -p karva_benchmark -- {{args}}
