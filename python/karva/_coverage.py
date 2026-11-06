"""Run-scoped startup and exit hooks for the native descendant tracer."""

import atexit
import importlib.machinery
import importlib.util
import json
import os
import signal
import sys
import tempfile
import uuid
from pathlib import Path
from typing import NoReturn

_BOOTSTRAP = "KARVA_COVERAGE_BOOTSTRAP"
_original_exit = os._exit


class _Hooks:
    def __init__(self, session, config, data_file, child):
        self.session = session
        self.directory = Path(data_file).parent
        self.active = True
        keys = (_BOOTSTRAP, "PYTHONPATH")
        self.environment = {key: os.environ.get(key) for key in keys}
        self.bootstrap_directory = None
        if not child:
            self.bootstrap_directory = tempfile.TemporaryDirectory(
                prefix="karva-coverage-"
            )
            bootstrap = Path(self.bootstrap_directory.name)
            payload = bootstrap / "config.json"
            payload.write_text(config, encoding="utf-8")
            (bootstrap / "sitecustomize.py").write_text(
                "from karva._coverage import bootstrap\nbootstrap()\n", encoding="utf-8"
            )
            os.environ[_BOOTSTRAP] = str(payload)
            paths = [str(bootstrap)]
            if os.environ.get("PYTHONPATH"):
                paths.append(os.environ["PYTHONPATH"])
            os.environ["PYTHONPATH"] = os.pathsep.join(paths)

        # ty treats a module's function definition as a nominal assignment target.
        os._exit = self.exit  # ty: ignore[invalid-assignment]
        sys.addaudithook(self.audit)
        if hasattr(os, "register_at_fork"):
            os.register_at_fork(after_in_child=self.after_fork)
        if child:
            atexit.register(self.save)
            self.install_signal()

    def shard(self):
        return str(self.directory / f"coverage-child-{uuid.uuid4().hex}.json")

    def save(self):
        if self.active:
            self.session.save()

    def audit(self, event, _args):
        if event == "os.exec":
            self.save()

    def exit(self, status: int) -> NoReturn:
        try:
            self.save()
        finally:
            _original_exit(status)

    def after_fork(self):
        if self.active:
            self.session.after_fork(self.shard())
            atexit.register(self.save)
            self.install_signal()

    def install_signal(self):
        if signal.getsignal(signal.SIGTERM) == signal.SIG_DFL:
            signal.signal(signal.SIGTERM, self.terminate)

    def terminate(self, signum, _frame):
        try:
            self.save()
        finally:
            signal.signal(signum, signal.SIG_DFL)
            os.kill(os.getpid(), signum)

    def close(self):
        self.active = False
        os._exit = _original_exit
        for key, value in self.environment.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
        if self.bootstrap_directory is not None:
            self.bootstrap_directory.cleanup()


def install(session, config, data_file, child):
    """Keep exit callbacks alive until the worker closes its coverage session."""
    return _Hooks(session, config, data_file, child)


def bootstrap():
    """Start only interpreters inheriting a live run's bootstrap environment."""
    payload = os.environ.get(_BOOTSTRAP)
    if payload and Path(payload).is_file():
        from karva._karva import coverage_child_start

        config = Path(payload).read_text(encoding="utf-8")
        directory = Path(json.loads(config)["data_file"]).parent
        shard = directory / f"coverage-child-{uuid.uuid4().hex}.json"
        install(coverage_child_start(config, str(shard)), config, str(shard), True)

    # Respect an application's own sitecustomize after our run-scoped bootstrap.
    directory = str(Path(__file__).parent) if not payload else str(Path(payload).parent)
    paths = [path for path in sys.path if path != directory]
    spec = importlib.machinery.PathFinder.find_spec("sitecustomize", paths)
    if spec and spec.loader:
        module = importlib.util.module_from_spec(spec)
        sys.modules["sitecustomize"] = module
        spec.loader.exec_module(module)
