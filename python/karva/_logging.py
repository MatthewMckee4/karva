"""One logging capture shared by the runner and caplog for each attempt."""

import logging
from collections.abc import Callable


class _LogCapture(logging.Handler):
    def __init__(self) -> None:
        super().__init__(logging.NOTSET)
        self.records: list[logging.LogRecord] = []
        self.captured: list[
            tuple[str, str, str, int, str, str, str, int, str | None]
        ] = []
        self.phase: str = "setup"

    def emit(self, record: logging.LogRecord) -> None:
        record.message = record.getMessage()
        self.records.append(record)
        exception = (
            logging.Formatter().formatException(record.exc_info)
            if record.exc_info
            else record.exc_text
        )
        self.captured.append(
            (
                self.phase,
                str(record.created),
                record.name,
                record.levelno,
                record.levelname,
                record.message,
                record.pathname,
                record.lineno,
                exception,
            )
        )

    def start(self) -> None:
        global _active
        _active = self
        self._basic_config: Callable[..., None] = logging.basicConfig
        vars(logging)["basicConfig"] = self._configure
        logging.getLogger().addHandler(self)

    def _configure(self, **kwargs: object) -> None:
        """Hide capture from basicConfig and protect it from force=True."""
        root = logging.getLogger()
        root.removeHandler(self)
        try:
            self._basic_config(**kwargs)
        finally:
            root.addHandler(self)

    def finish(self) -> None:
        global _active
        vars(logging)["basicConfig"] = self._basic_config
        logging.getLogger().removeHandler(self)
        _active = None
        self.close()


_active: _LogCapture | None = None
