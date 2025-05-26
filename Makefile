dev:
	uv sync --all-extras
	uv run pre-commit install
	uv pip install -e .

docs:
	uv run mkdocs build

docs-serve: dev
	uv run mkdocs serve

clean:
	git clean -xdf

.PHONY: dev pre-commit build clean docs
