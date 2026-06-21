"""Parser stub — to be ported from src-tauri/src/commands/import/${name}.rs."""
import sqlite3


def parse(file_path: str, password: str | None = None) -> dict:
    raise NotImplementedError(
        f"Parser '{__name__}' not yet ported from Rust. "
        "See src-tauri/src/commands/import/ for the reference implementation."
    )


def import_(conn: sqlite3.Connection, account_id: int, data: dict, batch_id: int | None = None) -> dict:
    raise NotImplementedError(f"Importer '{__name__}' not yet ported from Rust.")
