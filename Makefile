dev:
	uv run pre-commit install

docs:
	uv run mkdocs build

docs-serve: dev
	uv run mkdocs serve

clean:
	git clean -xdf

.PHONY: dev pre-commit build clean docs
