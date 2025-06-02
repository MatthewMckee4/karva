dev:
	pre-commit install
	uv venv
	uv pip install tbump

build:
	uv venv
	cargo build

docs:
	uv run mkdocs build

docs-serve: dev
	uv run mkdocs serve

clean:
	git clean -xdf

check:
	cargo check --all-targets --all-features

lint:
	cargo clippy --all-targets --all-features -- -D warnings

format:
	cargo +nightly fmt

ITERATIONS ?= 1
NUM_TESTS ?= 10000

benchmark: build
	cd scripts/benchmark && uv sync --all-extras && uv run main.py --iterations $(ITERATIONS) --num-tests $(NUM_TESTS) --run-test

flame:
	cd scripts/benchmark && uv sync --all-extras && uv run main.py --keep-test-file --num-tests 1000 && cd ../..
	sudo sysctl kernel.perf_event_paranoid=-1
	cargo flamegraph -- test scripts/benchmark/test_many_assertions.py

N ?= 1000

temp-test-dir:
	rm -rf temp_test_dir
	mkdir -p temp_test_dir
	for i in $$(seq 1 $(N)); do \
		printf "def test_pass_%d():\n    assert False\n" $$i > temp_test_dir/test_$$i.py; \
	done


.PHONY: dev pre-commit build clean docs benchmark build
