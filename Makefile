dev:
	pre-commit install

build:
	uv venv
	cargo build

docs:
	uv run mkdocs build

docs-serve: dev
	uv run mkdocs serve

clean:
	git clean -xdf

ITERATIONS ?= 1
NUM_TESTS ?= 10000

benchmark: build
	cd scripts/benchmark && uv sync --all-extras && uv run main.py --iterations $(ITERATIONS) --num-tests $(NUM_TESTS)

.PHONY: dev pre-commit build clean docs benchmark build
