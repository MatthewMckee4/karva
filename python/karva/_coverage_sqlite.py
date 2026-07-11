"""Write Karva coverage data as a coverage.py-compatible SQLite database."""

from __future__ import annotations

import json
import sqlite3
import sys
from typing import Any


def main() -> int:
    output = sys.argv[1]
    schema_version = int(sys.argv[2])
    payload: dict[str, Any] = json.load(sys.stdin)

    conn = sqlite3.connect(output)
    try:
        conn.executescript(
            """
            CREATE TABLE coverage_schema (
                version integer
            );
            CREATE TABLE meta (
                key text,
                value text,
                unique (key)
            );
            CREATE TABLE file (
                id integer primary key,
                path text,
                unique (path)
            );
            CREATE TABLE context (
                id integer primary key,
                context text,
                unique (context)
            );
            CREATE TABLE line_bits (
                file_id integer,
                context_id integer,
                numbits blob,
                foreign key (file_id) references file (id),
                foreign key (context_id) references context (id),
                unique (file_id, context_id)
            );
            CREATE TABLE arc (
                file_id integer,
                context_id integer,
                fromno integer,
                tono integer,
                foreign key (file_id) references file (id),
                foreign key (context_id) references context (id),
                unique (file_id, context_id, fromno, tono)
            );
            CREATE TABLE tracer (
                file_id integer primary key,
                tracer text,
                foreign key (file_id) references file (id)
            );
            """
        )
        with conn:
            conn.execute(
                "INSERT INTO coverage_schema (version) VALUES (?)", (schema_version,)
            )
            conn.execute(
                "INSERT INTO meta (key, value) VALUES (?, ?)", ("version", "karva")
            )
            conn.execute(
                "INSERT INTO meta (key, value) VALUES (?, ?)", ("has_arcs", "0")
            )

            context_ids: dict[str, int] = {}
            for file_row in payload["files"]:
                cursor = conn.execute(
                    "INSERT INTO file (path) VALUES (?)", (file_row["path"],)
                )
                file_id = cursor.lastrowid
                if file_id is None:
                    raise RuntimeError("sqlite did not return a file id")
                for context_row in file_row["contexts"]:
                    context = context_row["context"]
                    context_id = context_ids.get(context)
                    if context_id is None:
                        cursor = conn.execute(
                            "INSERT INTO context (context) VALUES (?)",
                            (context,),
                        )
                        context_id = cursor.lastrowid
                        if context_id is None:
                            raise RuntimeError("sqlite did not return a context id")
                        context_ids[context] = context_id
                    conn.execute(
                        "INSERT INTO line_bits (file_id, context_id, numbits) VALUES (?, ?, ?)",
                        (
                            file_id,
                            context_id,
                            sqlite3.Binary(bytes(context_row["numbits"])),
                        ),
                    )
    finally:
        conn.close()

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
