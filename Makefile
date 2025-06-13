dev:
	pre-commit install
	uv venv
	uv pip install tbump

build:
	uv venv
	cargo build

docs:
	uv run mkdocs build

docs-serve:
	uv run mkdocs serve

clean:
	git clean -xdf

check:
	cargo check --all-targets --all-features

lint:
	cargo clippy --all-targets --all-features -- -D warnings

format:
	cargo +nightly fmt
	cargo sort

ITERATIONS ?= 1
NUM_TESTS ?= 10000

benchmark: build
	cd scripts/benchmark && uv sync --all-extras --no-install-project && uv pip install -e ../../  && uv run main.py --iterations $(ITERATIONS) --num-tests $(NUM_TESTS) --run-test

flame:
	$(MAKE) temp-test-dir N=10000
	sudo sysctl kernel.perf_event_paranoid=-1
	cargo flamegraph --bin karva -- test temp_test_dir

N ?= 1000

temp-test-dir:
	rm -rf temp_test_dir

	mkdir -p temp_test_dir
	for i in $$(seq 1 $(N)); do \
		printf "def test_pass_%d():\n    assert True\n" $$i > temp_test_dir/test_$$i.py; \
	done


.PHONY: dev pre-commit build clean docs benchmark build
