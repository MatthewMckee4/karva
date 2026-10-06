"""First party assertion rewriting used by the worker runtime.

The hook is deliberately limited to Python files below the project root. It
rewrites only ``assert`` statements and keeps the native assertion expression
in place, so Python's normal scope, short-circuit, and exception semantics are
left to the interpreter.
"""

from __future__ import annotations

import ast
import builtins
import difflib
import importlib.abc
import importlib.machinery
import os
import sys
import tokenize
from collections.abc import Sequence
from types import FrameType, ModuleType
from typing import cast

_FINDER: _Finder | None = None
# Receipt: the existing parameter renderer truncates at 30 characters; a
# measured 240-character assertion body fits eight short scalar entries while
# keeping terminal and machine reports compact.
_MAX_ITEMS = 8
_MAX_TEXT = 240
# Receipt: eight entries across eight nested levels gives this 64-node cap.
_MAX_REPR_NODES = _MAX_ITEMS * _MAX_ITEMS
_MISSING = object()
_METADATA: dict[str, dict[int, list[tuple[int, str, tuple[tuple[str, str], ...]]]]] = {}


def install(root: str) -> None:
    """Install assertion instrumentation for source modules below ``root``."""
    global _FINDER
    root = os.path.realpath(root)
    if _FINDER is not None:
        _FINDER.roots.add(root)
        return
    _FINDER = _Finder({root})
    sys.meta_path.insert(0, _FINDER)


class _Finder(importlib.abc.MetaPathFinder):
    def __init__(self, roots: set[str]) -> None:
        self.roots = roots

    def find_spec(
        self,
        fullname: str,
        path: Sequence[str] | None,
        target: ModuleType | None = None,
    ):
        spec = importlib.machinery.PathFinder.find_spec(fullname, path, target)
        if spec is None or spec.origin is None or spec.loader is None:
            return None
        origin = os.path.realpath(spec.origin)
        if (
            not origin.endswith(".py")
            or not any(_under(origin, root) for root in self.roots)
            or _is_environment_path(origin, self.roots)
            or not isinstance(spec.loader, importlib.machinery.SourceFileLoader)
        ):
            return None
        spec.loader = _Loader(fullname, origin)
        return spec


class _Loader(importlib.machinery.SourceFileLoader):
    def __init__(self, name: str, filename: str) -> None:
        super().__init__(name, filename)

    def exec_module(self, module: ModuleType) -> None:
        with tokenize.open(self.path) as source_file:
            source = source_file.read()
        tree = ast.parse(source, self.path)
        value_prefix, missing_name = _helper_names(tree)
        transformer = _Transformer(source, value_prefix, missing_name)
        tree = transformer.visit(tree)
        ast.fix_missing_locations(tree)
        _METADATA[self.path] = transformer.assertions
        module.__dict__[missing_name] = _MISSING
        exec(compile(tree, self.path, "exec"), module.__dict__)


class _Transformer(ast.NodeTransformer):
    def __init__(self, source: str, value_prefix: str, missing_name: str) -> None:
        self.source = source
        self.value_prefix = value_prefix
        self.missing_name = missing_name
        self.value_index = 0
        self.bindings: list[tuple[str, str]] = []
        self.optional_values: list[str] = []
        self.assertions: dict[
            int, list[tuple[int, str, tuple[tuple[str, str], ...]]]
        ] = {}

    def visit_Assert(self, node: ast.Assert) -> ast.stmt | list[ast.stmt]:
        self.bindings = []
        self.optional_values = []
        test = self._instrument_test(node.test)
        source = ast.get_source_segment(self.source, node.test) or ast.unparse(
            node.test
        )
        self.assertions.setdefault(node.lineno, []).append(
            (node.col_offset, source.strip(), tuple(self.bindings))
        )
        initializers = [
            ast.Compare(
                left=ast.NamedExpr(
                    target=ast.Name(id=name, ctx=ast.Store()),
                    value=ast.Name(id=self.missing_name, ctx=ast.Load()),
                ),
                ops=[ast.IsNot()],
                comparators=[ast.Name(id=self.missing_name, ctx=ast.Load())],
            )
            for name in self.optional_values
        ]
        if initializers:
            test = ast.BoolOp(op=ast.Or(), values=[*initializers, test])
        assertion = ast.copy_location(ast.Assert(test=test, msg=node.msg), node)
        cleanup = ast.copy_location(
            ast.Delete(
                targets=[ast.Name(id=name, ctx=ast.Del()) for _, name in self.bindings]
            ),
            node,
        )
        return ast.copy_location(
            ast.If(
                test=ast.Name(id="__debug__", ctx=ast.Load()),
                body=[assertion, cleanup],
                orelse=[],
            ),
            node,
        )

    def _instrument_test(self, node: ast.expr, may_skip: bool = False) -> ast.expr:
        if isinstance(node, ast.BoolOp):
            node.values = [
                self._instrument_test(value, may_skip or index > 0)
                for index, value in enumerate(node.values)
            ]
            return node
        if isinstance(node, ast.Compare):
            node.left = self._capture(node.left, may_skip)
            node.comparators = [
                self._capture(value, may_skip or index > 0)
                for index, value in enumerate(node.comparators)
            ]
            return node
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.Not):
            node.operand = self._instrument_test(node.operand, may_skip)
            return node
        return self._capture(node, may_skip)

    def _capture(self, node: ast.expr, may_skip: bool) -> ast.expr:
        source = ast.get_source_segment(self.source, node) or ast.unparse(node)
        name = f"{self.value_prefix}{self.value_index}"
        self.value_index += 1
        self.bindings.append((source.strip(), name))
        if may_skip:
            self.optional_values.append(name)
        return ast.copy_location(
            ast.NamedExpr(
                target=ast.Name(id=name, ctx=ast.Store()),
                value=node,
            ),
            node,
        )


def _helper_names(tree: ast.AST) -> tuple[str, str]:
    names = {node.id for node in ast.walk(tree) if isinstance(node, ast.Name)}
    names.update(node.arg for node in ast.walk(tree) if isinstance(node, ast.arg))
    names.update(
        node.name
        for node in ast.walk(tree)
        if isinstance(node, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef))
    )
    names.update(
        name
        for node in ast.walk(tree)
        if isinstance(node, (ast.Global, ast.Nonlocal))
        for name in node.names
    )
    names.update(
        alias.asname or alias.name.partition(".")[0]
        for node in ast.walk(tree)
        if isinstance(node, (ast.Import, ast.ImportFrom))
        for alias in node.names
    )
    names.update(
        node.name
        for node in ast.walk(tree)
        if isinstance(node, ast.ExceptHandler) and node.name
    )
    suffix = ""
    while True:
        value_prefix = f"_karva_value_{suffix}"
        missing = f"_karva_missing_{suffix}"
        if missing not in names and not any(
            name.startswith(value_prefix) for name in names
        ):
            return value_prefix, missing
        suffix += "_"


def _under(path: str, root: str) -> bool:
    try:
        return os.path.commonpath((path, root)) == root
    except ValueError:
        return False


def _is_environment_path(path: str, roots: set[str]) -> bool:
    for root in roots:
        if _under(path, root):
            relative = os.path.relpath(path, root).split(os.sep)
            if any(
                part in {".venv", "venv", ".tox", ".nox", "site-packages"}
                for part in relative
            ):
                return True
    return False


def consume_explanation(error: object) -> str | None:
    """Return assertion explanation derived from the failing traceback."""
    return _native_explanation(error)


def _native_explanation(error: object) -> str | None:
    traceback = getattr(error, "__traceback__", None)
    while traceback is not None:
        if traceback.tb_next is None:
            frame = traceback.tb_frame
            assertions = _METADATA.get(os.path.realpath(frame.f_code.co_filename))
            if assertions is not None:
                entries = assertions.get(traceback.tb_lineno)
                if entries:
                    _, source, bindings = _select_assertion(
                        frame, traceback.tb_lasti, entries
                    )
                    records = [
                        (label, frame.f_locals[name])
                        for label, name in bindings
                        if name in frame.f_locals
                        and frame.f_locals[name] is not _MISSING
                    ]
                    try:
                        return _explanation(source, records)
                    except BaseException:
                        return f"assert {_clip(source)}"
        traceback = traceback.tb_next
    return None


def _select_assertion(
    frame: FrameType,
    lasti: int,
    entries: list[tuple[int, str, tuple[tuple[str, str], ...]]],
) -> tuple[int, str, tuple[tuple[str, str], ...]]:
    if len(entries) == 1:
        return entries[0]
    positions = getattr(frame.f_code, "co_positions", lambda: ())()
    position = next(
        (item for index, item in enumerate(positions) if index == lasti // 2),
        None,
    )
    if position is None or position[2] is None:
        return entries[0]
    return min(entries, key=lambda entry: abs(entry[0] - position[2]))


def _explanation(source: str, records: list[tuple[str, object]]) -> str:
    source = _clip(source)
    preserve_same_label = _is_identity_comparison(source)
    values: list[tuple[str, object]] = []
    for item in reversed(records):
        if not any(
            existing[0] == item[0]
            and (not preserve_same_label or existing[1] is item[1])
            for existing in values
        ):
            values.append(item)
        if len(values) == 2:
            break
    values.reverse()
    lines = [f"assert {source}"]
    if values:
        if len(values) == 2 and _is_comparison(source):
            diff = _focused_diff(values[0][1], values[1][1])
            if diff:
                lines.extend(("", "Differing values:"))
                lines.extend(f"  {line}" for line in diff)
                return "\n".join(lines)
        values = [
            (label, value)
            for label, value in values
            if not _is_literal_duplicate(label, value)
        ]
        if values:
            lines.extend(("", "Differing values:"))
            label, value = values[-1]
            if len(values) == 1:
                lines.append(f"  {_clip(label)}: {_safe_repr(value)}")
            else:
                rendered = [_safe_repr(value) for _, value in values]
                if (
                    _is_identity_comparison(source)
                    and rendered[0] == rendered[1]
                    and values[0][1] is not values[1][1]
                ):
                    lines.extend(
                        f"  {_clip(label)}: {representation} (distinct objects)"
                        for (label, _), representation in zip(
                            values, rendered, strict=True
                        )
                    )
                else:
                    lines.extend(
                        f"  {_clip(label)}: {representation}"
                        for (label, _), representation in zip(
                            values, rendered, strict=True
                        )
                    )
    return "\n".join(lines)


def _is_comparison(source: str) -> bool:
    try:
        return isinstance(ast.parse(source, mode="eval").body, ast.Compare)
    except SyntaxError:
        return False


def _is_identity_comparison(source: str) -> bool:
    try:
        expression = ast.parse(source, mode="eval").body
    except SyntaxError:
        return False
    return isinstance(expression, ast.Compare) and any(
        isinstance(operator, (ast.Is, ast.IsNot)) for operator in expression.ops
    )


def _is_literal_duplicate(label: str, value: object) -> bool:
    try:
        literal = ast.literal_eval(label)
    except BaseException:
        return False
    return type(literal) is type(value) and _safe_repr(literal) == _safe_repr(value)


def _focused_diff(left: object, right: object) -> list[str]:
    if (
        type(left) is str
        and type(right) is str
        and len(left) <= _MAX_TEXT * 4
        and len(right) <= _MAX_TEXT * 4
    ):
        diff = list(difflib.ndiff(left.splitlines(), right.splitlines()))
        if diff and len("\n".join(diff)) <= _MAX_TEXT:
            return ["string diff:", *diff]
    if type(left) is bytes and type(right) is bytes and left != right:
        return [f"actual: {_safe_repr(left)}", f"expected: {_safe_repr(right)}"]
    if (
        type(left) is dict
        and type(right) is dict
        and _safe_data(left)
        and _safe_data(right)
    ):
        left_dict = cast(dict[object, object], left)
        right_dict = cast(dict[object, object], right)
        changed = [
            key
            for key in left_dict.keys() & right_dict.keys()
            if left_dict[key] != right_dict[key]
        ]
        missing = list(left_dict.keys() - right_dict.keys())
        extra = list(right_dict.keys() - left_dict.keys())
        if changed or missing or extra:
            result = []
            for key in changed[:_MAX_ITEMS]:
                result.append(
                    f"actual[{_safe_repr(key)}]: {_safe_repr(left_dict[key])}"
                )
                result.append(
                    f"expected[{_safe_repr(key)}]: {_safe_repr(right_dict[key])}"
                )
            if missing:
                result.append(f"missing keys: {_safe_repr(missing)}")
            if extra:
                result.append(f"unexpected keys: {_safe_repr(extra)}")
            return result
    if (
        type(left) in (list, tuple)
        and type(right) is type(left)
        and _safe_data(left)
        and _safe_data(right)
    ):
        left_sequence = cast(Sequence[object], left)
        right_sequence = cast(Sequence[object], right)
        for index, (actual, expected) in enumerate(
            zip(left_sequence, right_sequence, strict=False)
        ):
            if actual != expected:
                return [
                    f"actual[{index}]: {_safe_repr(actual)}",
                    f"expected[{index}]: {_safe_repr(expected)}",
                ]
    if (
        type(left) in (set, frozenset)
        and type(right) is type(left)
        and _safe_data(left)
        and _safe_data(right)
    ):
        left_set = cast(set[object] | frozenset[object], left)
        right_set = cast(set[object] | frozenset[object], right)
        missing = left_set - right_set
        extra = right_set - left_set
        if missing or extra:
            return [
                f"unexpected: {_safe_repr(missing)}",
                f"missing: {_safe_repr(extra)}",
            ]
    return []


def _safe_data(value: object, depth: int = 0) -> bool:
    # Receipt: the eight-entry display cap bounds this validation to 64 nodes;
    # nested values beyond that are rendered by the safe fallback instead.
    return _safe_data_bounded(value, depth, [_MAX_ITEMS * _MAX_ITEMS])


def _safe_data_bounded(value: object, depth: int, budget: list[int]) -> bool:
    if depth > _MAX_ITEMS:
        return False
    budget[0] -= 1
    if budget[0] < 0:
        return False
    value_type = type(value)
    if value is None or value_type in (bool, int, float, complex, str, bytes):
        return True
    if value_type is dict:
        mapping = cast(dict[object, object], value)
        return len(mapping) <= _MAX_ITEMS and all(
            _safe_data_bounded(key, depth + 1, budget)
            and _safe_data_bounded(item, depth + 1, budget)
            for key, item in mapping.items()
        )
    if value_type in (list, tuple, set, frozenset):
        sequence = cast(Sequence[object] | set[object] | frozenset[object], value)
        return len(sequence) <= _MAX_ITEMS and all(
            _safe_data_bounded(item, depth + 1, budget) for item in sequence
        )
    return False


def _safe_repr(
    value: object,
    seen: set[int] | None = None,
    depth: int = 0,
    budget: list[int] | None = None,
) -> str:
    try:
        return _safe_repr_inner(value, seen, depth, budget)
    except BaseException:
        return "<unavailable>"


def _safe_repr_inner(
    value: object,
    seen: set[int] | None,
    depth: int,
    budget: list[int] | None,
) -> str:
    """Represent builtins without dispatching arbitrary user ``__repr__``."""
    seen = set() if seen is None else seen
    budget = [_MAX_REPR_NODES] if budget is None else budget
    if depth > _MAX_ITEMS or budget[0] <= 0:
        return _type_fallback(value)
    budget[0] -= 1
    value_type = type(value)
    if value_type is str:
        text = cast(str, value)
        truncated = len(text) > _MAX_TEXT
        return _bounded_repr(builtins.repr(text[:_MAX_TEXT]), truncated)
    if value_type is bytes:
        data = cast(bytes, value)
        truncated = len(data) > _MAX_TEXT
        return _bounded_repr(builtins.repr(data[:_MAX_TEXT]), truncated)
    if value is None or value_type in (bool, float, complex):
        return _clip(builtins.repr(value))
    if value_type is int:
        integer = cast(int, value)
        bits = integer.bit_length()
        if bits > _MAX_TEXT * 8:
            return f"<int {bits} bits>"
        return builtins.repr(integer)
    if value_type in (list, tuple, set, frozenset, dict):
        identity = id(value)
        if identity in seen:
            return "<recursive>"
        seen.add(identity)
        try:
            if value_type is dict:
                mapping = cast(dict[object, object], value)
                rendered = []
                for index, (key, item) in enumerate(mapping.items()):
                    if index == _MAX_ITEMS:
                        break
                    rendered.append(
                        f"{_safe_repr(key, seen, depth + 1, budget)}: "
                        f"{_safe_repr(item, seen, depth + 1, budget)}"
                    )
                suffix = ", ..." if len(mapping) > _MAX_ITEMS else ""
                return _clip("{" + ", ".join(rendered) + suffix + "}")
            sequence = cast(Sequence[object] | set[object] | frozenset[object], value)
            rendered = []
            for index, item in enumerate(sequence):
                if index == _MAX_ITEMS:
                    break
                rendered.append(_safe_repr(item, seen, depth + 1, budget))
            suffix = ", ..." if len(sequence) > _MAX_ITEMS else ""
            if value_type is tuple:
                comma = "," if len(sequence) == 1 else ""
                return _clip("(" + ", ".join(rendered) + suffix + comma + ")")
            if value_type is set:
                return _clip("{" + ", ".join(rendered) + suffix + "}")
            if value_type is frozenset:
                return _clip("frozenset({" + ", ".join(rendered) + suffix + "})")
            return _clip("[" + ", ".join(rendered) + suffix + "]")
        finally:
            seen.remove(identity)
    return _type_fallback(value)


def _clip(value: str) -> str:
    return value if len(value) <= _MAX_TEXT else value[: _MAX_TEXT - 3] + "..."


def _bounded_repr(value: str, truncated: bool = False) -> str:
    closing = value[-1:] if value[-1:] in {"'", '"'} else ""
    if not truncated and len(value) <= _MAX_TEXT:
        return value
    body = value[: -len(closing)] if closing else value
    limit = _MAX_TEXT - len(closing) - 3
    return body[:limit] + "..." + closing


def _type_fallback(value: object) -> str:
    value_type = type(value)
    try:
        module = type.__getattribute__(value_type, "__module__")
        qualname = type.__getattribute__(value_type, "__qualname__")
    except BaseException:
        return "<object>"
    if type(module) is not str or type(qualname) is not str:
        return "<object>"
    return f"<{module}.{qualname} object>"
