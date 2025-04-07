dev:
	uv sync --all-extras
	uv run pre-commit install
	uv pip install -e .

pre-commit:
	uv run pre-commit run --all-files

build:
	uv run python -m build

docs:
	uv run mkdocs build

docs-serve: dev
	uv run mkdocs serve

clean:
	rm -rf site
	rm -rf .venv
	find src -name "*.c" | xargs rm -rf
	find src -name "*.pyc" | xargs rm -rf
	find src -name "*.pyd" | xargs rm -rf
	find . -name "*.egg_info" | xargs rm -rf
	find . -name ".ipynb_checkpoints" | xargs rm -rf
	find . -name ".mypy_cache" | xargs rm -rf
	find . -name ".pytest_cache" | xargs rm -rf
	find . -name ".ruff_cache" | xargs rm -rf
	find . -name "__pycache__" | xargs rm -rf
	find . -name "build" | xargs rm -rf
	find . -name "builds" | xargs rm -rf
	find . -name "dist" -not -path "*node_modules*" | xargs rm -rf
	find . -name "*.egg-info" | xargs rm -rf

.PHONY: dev pre-commit build clean docs
