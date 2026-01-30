# https://just.systems

test *args:
    maturin build
    @if command -v cargo-nextest > /dev/null 2>&1; then \
        cargo nextest run {{args}}; \
    else \
        cargo test {{args}}; \
    fi
