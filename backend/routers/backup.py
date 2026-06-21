"""Backup export/import — port of src-tauri/src/commands/backup.rs.

.ptdata binary format (must stay byte-compatible with Rust):
  "PTDT"              4 bytes  magic
  version: u32 LE    4 bytes  = 2
  salt                16 bytes (Argon2 salt for wrapping the export key)
  wrap_nonce          12 bytes
  wrap_ct_len: u32 LE 4 bytes
  wrap_ciphertext     variable
  db_nonce            12 bytes
  db_len: u64 LE      8 bytes
  encrypted_db        variable
"""
import os
import sqlite3
import struct
from pathlib import Path
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException, Request
from pydantic import BaseModel

from core import auth as auth_module
from core import session as session_module
from routers.deps import get_conn

from cryptography.hazmat.primitives.ciphers.aead import AESGCM

router = APIRouter(tags=["backup"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]

_MAGIC   = b"PTDT"
_VERSION = 2


class ExportInput(BaseModel):
    pin: str
    password: str
    dest_path: str


class ImportInput(BaseModel):
    password: str
    src_path: str


def _require_session(request: Request) -> bytes:
    token = request.headers.get("X-Session-Token")
    if not token:
        raise HTTPException(status_code=401, detail="Missing X-Session-Token header")
    try:
        return session_module.get_key(token)
    except KeyError:
        raise HTTPException(status_code=401, detail="Invalid session token")


@router.post("/export")
def export_data(body: ExportInput, request: Request, conn: Conn):
    app_dir: Path = request.app.state.app_dir

    # Verify PIN
    try:
        auth_module.login(app_dir, body.pin)
    except ValueError:
        raise HTTPException(status_code=401, detail="Incorrect PIN")

    # Read the plain DB file (connection is still open — checkpoint first)
    conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    conn.commit()

    db_path = app_dir / "portfolio.db"
    plaintext_db = db_path.read_bytes()

    # Encrypt DB with a fresh random export key
    export_key = os.urandom(32)
    db_nonce   = os.urandom(12)
    encrypted_db = AESGCM(export_key).encrypt(db_nonce, plaintext_db, None)

    # Wrap export key with the user-supplied export password (Argon2id + AES-GCM)
    wrapped = auth_module.wrap_key(export_key, body.password)

    import base64
    salt_bytes     = base64.b64decode(wrapped["salt"])
    wrap_nonce     = base64.b64decode(wrapped["nonce"])
    wrap_ct        = base64.b64decode(wrapped["ciphertext"])

    # Write binary file
    with open(body.dest_path, "wb") as f:
        f.write(_MAGIC)
        f.write(struct.pack("<I", _VERSION))        # u32 LE
        f.write(salt_bytes)                          # 16 bytes
        f.write(wrap_nonce)                          # 12 bytes
        f.write(struct.pack("<I", len(wrap_ct)))     # u32 LE
        f.write(wrap_ct)
        f.write(db_nonce)                            # 12 bytes
        f.write(struct.pack("<Q", len(encrypted_db)))# u64 LE
        f.write(encrypted_db)

    return {"ok": True}


@router.post("/import")
def import_data(body: ImportInput, request: Request, conn: Conn):
    import base64

    app_dir: Path = request.app.state.app_dir

    data = Path(body.src_path).read_bytes()
    cur  = 0

    if data[cur:cur + 4] != _MAGIC:
        raise HTTPException(status_code=400, detail="Invalid file — not a .ptdata file")
    cur += 4

    version = struct.unpack_from("<I", data, cur)[0]; cur += 4
    if version != _VERSION:
        raise HTTPException(status_code=400, detail=f"Unsupported .ptdata version {version}")

    salt       = data[cur:cur + 16]; cur += 16
    wrap_nonce = data[cur:cur + 12]; cur += 12
    wct_len    = struct.unpack_from("<I", data, cur)[0]; cur += 4
    wrap_ct    = data[cur:cur + wct_len]; cur += wct_len
    db_nonce   = data[cur:cur + 12]; cur += 12
    db_len     = struct.unpack_from("<Q", data, cur)[0]; cur += 8
    encrypted_db = data[cur:cur + db_len]

    # Unwrap export key
    wrapped = {
        "salt":       base64.b64encode(salt).decode(),
        "nonce":      base64.b64encode(wrap_nonce).decode(),
        "ciphertext": base64.b64encode(wrap_ct).decode(),
    }
    try:
        export_key = auth_module.unwrap_key(wrapped, body.password)
    except ValueError:
        raise HTTPException(status_code=401, detail="Incorrect export password")

    # Decrypt DB
    try:
        plaintext_db = AESGCM(export_key).decrypt(db_nonce, encrypted_db, None)
    except Exception:
        raise HTTPException(status_code=400, detail="Decryption failed — corrupt file or wrong password")

    # Close current connection, write new DB, reopen
    master_key = _require_session(request)

    conn.close()
    request.app.state.conn = None

    db_path   = app_dir / "portfolio.db"
    temp_path = app_dir / "import_temp.db"

    temp_path.write_bytes(plaintext_db)
    for suffix in ("-wal", "-shm"):
        try:
            (app_dir / f"portfolio.db{suffix}").unlink()
        except FileNotFoundError:
            pass
    temp_path.rename(db_path)

    # Re-encrypt the newly placed DB under the current master key
    from core import db as db_module
    new_conn = db_module.open_database(app_dir, master_key)
    request.app.state.conn = new_conn

    return {"ok": True}
