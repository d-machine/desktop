import os
import sqlite3
from pathlib import Path

from cryptography.hazmat.primitives.ciphers.aead import AESGCM

from core.migrations import run_migrations

_NONCE_LEN = 12
_ENC_SUFFIX = ".enc"


def _plain_path(app_dir: Path) -> Path:
    return app_dir / "portfolio.db"


def _enc_path(app_dir: Path) -> Path:
    return app_dir / f"portfolio.db{_ENC_SUFFIX}"


def _encrypt_file(src: Path, dest: Path, master_key: bytes) -> None:
    nonce = os.urandom(_NONCE_LEN)
    plaintext = src.read_bytes()
    ciphertext = AESGCM(master_key).encrypt(nonce, plaintext, None)
    dest.write_bytes(nonce + ciphertext)


def _decrypt_file(src: Path, dest: Path, master_key: bytes) -> None:
    data = src.read_bytes()
    nonce, ciphertext = data[:_NONCE_LEN], data[_NONCE_LEN:]
    plaintext = AESGCM(master_key).decrypt(nonce, ciphertext, None)
    dest.write_bytes(plaintext)


def open_database(app_dir: Path, master_key: bytes) -> sqlite3.Connection:
    plain = _plain_path(app_dir)
    enc = _enc_path(app_dir)

    if plain.exists() and enc.exists():
        # Previous session crashed — encrypted backup exists, discard stale plaintext.
        plain.unlink()

    if enc.exists():
        _decrypt_file(enc, plain, master_key)
    # else: first run — SQLite will create the file on connect.

    conn = sqlite3.connect(str(plain), check_same_thread=False)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA foreign_keys=ON")
    conn.execute("PRAGMA busy_timeout=5000")
    conn.commit()

    run_migrations(conn)
    return conn


def close_database(app_dir: Path, master_key: bytes, conn: sqlite3.Connection) -> None:
    """Flush, close, encrypt, delete plaintext. Call on clean shutdown or lock."""
    try:
        conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        conn.commit()
    finally:
        conn.close()

    plain = _plain_path(app_dir)
    enc = _enc_path(app_dir)

    if plain.exists():
        _encrypt_file(plain, enc, master_key)
        plain.unlink()


def get_connection(app_dir: Path) -> sqlite3.Connection:
    """
    Return the already-open connection. Call open_database() once per session;
    then use this from routers to get the connection without re-opening.
    The actual open connection is stored on FastAPI app.state by main.py.
    This helper exists so routers can import a consistent accessor pattern.
    """
    raise RuntimeError("Use app.state.conn — do not call get_connection() directly")
