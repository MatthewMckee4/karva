"""Assertion rewriting for test modules."""

import ast
import io
import os
import sys
import tokenize
from collections.abc import Sequence
from importlib.abc import MetaPathFinder
from importlib.machinery import ModuleSpec, PathFinder, SourceFileLoader
from importlib.util import decode_source
from types import CodeType, ModuleType
from typing import Any


def _normalized_path(path: str) -> str:
    return os.path.normcase(os.path.realpath(path))


def _format_assertion(
    expression: str,
    local_vars: dict[str, Any],
    global_vars: dict[str, Any],
) -> str:
    values = global_vars | local_vars
    tokens = list(tokenize.generate_tokens(io.StringIO(expression).readline))
    line_offsets = [0]
    for line in expression.splitlines(keepends=True):
        line_offsets.append(line_offsets[-1] + len(line))

    replacements: list[tuple[int, int, str]] = []
    significant = [
        token
        for token in tokens
        if token.type
        not in {tokenize.ENCODING, tokenize.ENDMARKER, tokenize.INDENT, tokenize.DEDENT}
    ]
    for index, token in enumerate(significant):
        if (
            token.type != tokenize.NAME
            or token.string not in values
            or isinstance(values[token.string], ModuleType)
        ):
            continue
        previous = significant[index - 1].string if index else None
        following = (
            significant[index + 1].string if index + 1 < len(significant) else None
        )
        if previous == "." or following == "(":
            continue
        start = line_offsets[token.start[0] - 1] + token.start[1]
        end = line_offsets[token.end[0] - 1] + token.end[1]
        replacements.append((start, end, repr(values[token.string])))

    rendered = expression
    for start, end, value in reversed(replacements):
        rendered = f"{rendered[:start]}{value}{rendered[end:]}"
    return f"assert {rendered}"


class _AssertionTransformer(ast.NodeTransformer):
    def __init__(self, source: str) -> None:
        self.source = source

    def visit_Assert(self, node: ast.Assert) -> ast.Assert:
        self.generic_visit(node)
        if node.msg is not None:
            return node

        expression = ast.get_source_segment(self.source, node.test) or ast.unparse(
            node.test
        )
        node.msg = ast.Call(
            func=ast.Name(id="_karva_format_assertion", ctx=ast.Load()),
            args=[
                ast.Constant(value=expression),
                ast.Call(
                    func=ast.Name(id="locals", ctx=ast.Load()), args=[], keywords=[]
                ),
                ast.Call(
                    func=ast.Name(id="globals", ctx=ast.Load()), args=[], keywords=[]
                ),
            ],
            keywords=[],
        )
        return node


class _AssertionRewritingLoader(SourceFileLoader):
    def get_code(self, fullname: str) -> CodeType:
        del fullname
        text = decode_source(self.get_data(self.path))
        tree = _AssertionTransformer(text).visit(ast.parse(text, self.path))
        ast.fix_missing_locations(tree)
        return compile(tree, self.path, "exec", dont_inherit=True)

    def exec_module(self, module: ModuleType) -> None:
        module.__dict__["_karva_format_assertion"] = _format_assertion
        super().exec_module(module)


class _AssertionRewritingFinder(MetaPathFinder):
    def __init__(self, paths: set[str]) -> None:
        self.paths = paths

    def find_spec(
        self,
        fullname: str,
        path: Sequence[str] | None,
        target: ModuleType | None = None,
    ) -> ModuleSpec | None:
        spec = PathFinder.find_spec(fullname, path, target)
        if (
            spec is not None
            and spec.origin is not None
            and _normalized_path(spec.origin) in self.paths
            and isinstance(spec.loader, SourceFileLoader)
        ):
            spec.loader = _AssertionRewritingLoader(fullname, spec.origin)
        return spec


def register_assertion_rewrite(paths: list[str]) -> None:
    normalized = {_normalized_path(path) for path in paths}
    for finder in sys.meta_path:
        if isinstance(finder, _AssertionRewritingFinder):
            finder.paths.update(normalized)
            return
    sys.meta_path.insert(0, _AssertionRewritingFinder(normalized))
