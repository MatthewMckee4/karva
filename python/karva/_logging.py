"""One logging capture shared by the runner and caplog for each attempt."""

import logging


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
        logging.getLogger().addHandler(self)

    def finish(self) -> None:
        global _active
        logging.getLogger().removeHandler(self)
        _active = None
        self.close()


_active: _LogCapture | None = None
