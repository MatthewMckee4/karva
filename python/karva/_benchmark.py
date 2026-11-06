"""Callable-only microbenchmark sampling shared with the worker runtime.

The worker opens a session before fixture setup and drains it after each test
attempt. Only calls through Benchmark are timed; setup and teardown stay outside
the samples. All reported times use integer nanoseconds.
"""

from __future__ import annotations

import gc
import inspect
import json
import math
import os
import statistics
import time
from collections.abc import Callable
from typing import Any, TypeVar

T = TypeVar("T")

_session: _Session | None = None


def _options(options: dict[str, Any]) -> dict[str, Any]:
    # Sampling receipt, CPython 3.12 on macOS arm64: perf_counter_ns cost 37 ns
    # with 42 ns resolution. A 1 ms batch puts two clock reads below 0.008%.
    # The 0.1 s target yielded 58 noop rounds and 55 int("42") rounds; a 25 ms
    # sleep exercised the five-round floor in 210 ms including warmup/calibration.
    # These are sampling targets, not capacity limits; slow calls finish normally.
    defaults: dict[str, Any] = {
        "group": None,
        "min_rounds": 5,
        "min_time": 0.001,
        "max_time": 0.1,
        "warmup": 1,
        "rounds": None,
        "iterations": None,
        "disable_gc": False,
    }
    for key in options:
        if key not in defaults:
            raise ValueError(f"unknown benchmark option: {key}")
    defaults.update(options)
    for key in ("min_rounds", "warmup", "rounds", "iterations"):
        value = defaults[key]
        if value is None and key in ("rounds", "iterations"):
            continue
        # pytest-benchmark also accepts warmup=True/False.
        if key == "warmup" and isinstance(value, bool):
            defaults[key] = int(value)
            continue
        minimum = 0 if key == "warmup" else 1
        if type(value) is not int or value < minimum:
            raise ValueError(
                f"benchmark {key} must be an integer >= {minimum}; requested {value!r}"
            )
    for key in ("min_time", "max_time"):
        value = defaults[key]
        if (
            isinstance(value, bool)
            or not isinstance(value, (float, int))
            or not math.isfinite(value)
            or value <= 0
        ):
            raise ValueError(
                f"benchmark {key} must be finite and > 0 seconds; requested {value!r}"
            )
    if defaults["min_time"] > defaults["max_time"]:
        raise ValueError(
            f"benchmark min_time requested {defaults['min_time']}s "
            f"exceeds max_time {defaults['max_time']}s"
        )
    if defaults["group"] is not None and not isinstance(defaults["group"], str):
        raise TypeError("benchmark group must be a string or None")
    if not isinstance(defaults["disable_gc"], bool):
        raise TypeError("benchmark disable_gc must be a boolean")
    return defaults


class _Session:
    def __init__(self, disabled: bool, options: dict[str, Any]) -> None:
        self.disabled = disabled
        self.options = _options(options)
        self.results: list[dict[str, Any]] = []


def _start(disabled: bool, options: dict[str, Any]) -> None:
    global _session
    _session = None
    _session = _Session(disabled, options)


def _finish() -> str:
    global _session
    session, _session = _session, None
    return json.dumps(session.results if session else [])


class Benchmark:
    """Measure a synchronous callable and return its last result.

    Each fixture may be used once, including as a decorator. The callable must
    tolerate repeated invocation. Statistics describe per-call round averages,
    not individual call latency. ``stats`` contains the same data as the report.
    """

    def __init__(self) -> None:
        if _session is None:
            raise RuntimeError(
                "benchmark fixture requires an active Karva test attempt"
            )
        self._session = _session
        self._used = False
        self.stats: dict[str, Any] | None = None

    def __repr__(self) -> str:
        return "<Benchmark>"

    def __call__(self, function: Callable[..., T], /, *args: Any, **kwargs: Any) -> T:
        if self._used:
            raise RuntimeError(
                "benchmark fixture may only measure one callable per test"
            )
        self._used = True
        if inspect.iscoroutinefunction(function) or inspect.isasyncgenfunction(
            function
        ):
            raise TypeError("benchmark requires a synchronous callable")
        session = self._session
        options = session.options
        workers = int(os.environ.get("KARVA_TEST_THREADS", "1"))
        if not session.disabled and workers > 1:
            raise RuntimeError(
                f"benchmark worker budget exceeded: requested {workers}, limit 1; "
                "run with --num-workers=1"
            )
        record: dict[str, Any] = {
            "group": options["group"],
            "disabled": session.disabled,
            "options": options.copy(),
            "rounds": 0,
            "iterations": 1,
            "warmup_calls": 0,
            "calibration_calls": 0,
            "measurement_calls": 0,
            "total_calls": 0,
            "elapsed_ns": 0,
            "samples_ns": [],
            "min_ns": None,
            "median_ns": None,
            "mean_ns": None,
            "stddev_ns": None,
            "stop_reason": "exception",
        }
        self.stats = record
        session.results.append(record)
        started = time.perf_counter_ns()
        gc_enabled = gc.isenabled()

        def batch(iterations: int, phase: str) -> tuple[T, int]:
            start = time.perf_counter_ns()
            for index in range(iterations):
                try:
                    result = function(*args, **kwargs)
                except BaseException:
                    record[phase] += index + 1
                    record["total_calls"] += index + 1
                    raise
            elapsed = time.perf_counter_ns() - start
            record[phase] += iterations
            record["total_calls"] += iterations
            if inspect.isawaitable(result) or inspect.isasyncgen(result):
                if inspect.iscoroutine(result):
                    result.close()
                raise TypeError(
                    "benchmark callable returned an awaitable; "
                    "use a synchronous callable"
                )
            return result, elapsed

        try:
            if session.disabled:
                result, _ = batch(1, "measurement_calls")
                record["stop_reason"] = "disabled"
                return result
            if options["disable_gc"]:
                gc.disable()
            if options["warmup"]:
                batch(options["warmup"], "warmup_calls")
            iterations = options["iterations"]
            max_time_ns = options["max_time"] * 1e9
            if iterations is None:
                iterations = 1
                while True:
                    _, elapsed = batch(iterations, "calibration_calls")
                    if (
                        elapsed >= options["min_time"] * 1e9
                        or time.perf_counter_ns() - started >= max_time_ns
                    ):
                        break
                    iterations *= 2
            record["iterations"] = iterations
            samples = record["samples_ns"]
            while True:
                result, elapsed = batch(iterations, "measurement_calls")
                samples.append(elapsed / iterations)
                record["rounds"] += 1
                if options["rounds"] is not None:
                    if record["rounds"] >= options["rounds"]:
                        record["stop_reason"] = "rounds"
                        break
                elif (
                    record["rounds"] >= options["min_rounds"]
                    and time.perf_counter_ns() - started >= max_time_ns
                ):
                    record["stop_reason"] = "max_time"
                    break
            return result
        finally:
            record["elapsed_ns"] = time.perf_counter_ns() - started
            if options["disable_gc"]:
                if gc_enabled:
                    gc.enable()
                else:
                    gc.disable()
            samples = record["samples_ns"]
            if samples:
                record["min_ns"] = min(samples)
                record["median_ns"] = statistics.median(samples)
                record["mean_ns"] = statistics.mean(samples)
                record["stddev_ns"] = (
                    statistics.stdev(samples) if len(samples) > 1 else 0.0
                )
                for key in ("min_ns", "median_ns", "mean_ns", "stddev_ns"):
                    record[key] = round(record[key])
                record["samples_ns"] = [round(sample) for sample in samples]
