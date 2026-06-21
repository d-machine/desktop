import base64
import json
import os
import secrets
from pathlib import Path

from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.argon2 import Argon2id

# Must match src-tauri/src/auth/crypto.rs exactly so existing auth.json files work.
_M_COST = 65536  # 64 MB
_T_COST = 3
_P_COST = 4
_KEY_LEN = 32
_NONCE_LEN = 12
_SALT_LEN = 16


def generate_master_key() -> bytes:
    return secrets.token_bytes(_KEY_LEN)


def derive_key(secret: str, salt: bytes) -> bytes:
    kdf = Argon2id(
        salt=salt,
        length=_KEY_LEN,
        iterations=_T_COST,
        lanes=_P_COST,
        memory_cost=_M_COST,
    )
    return kdf.derive(secret.encode())


def wrap_key(master_key: bytes, secret: str) -> dict:
    """Encrypt master_key with secret. Returns {salt, nonce, ciphertext} as base64 strings."""
    salt = os.urandom(_SALT_LEN)
    wrapping_key = derive_key(secret, salt)
    nonce = os.urandom(_NONCE_LEN)
    ciphertext = AESGCM(wrapping_key).encrypt(nonce, master_key, None)
    return {
        "salt": base64.b64encode(salt).decode(),
        "nonce": base64.b64encode(nonce).decode(),
        "ciphertext": base64.b64encode(ciphertext).decode(),
    }


def unwrap_key(wrapped: dict, secret: str) -> bytes:
    """Decrypt master_key from a WrappedKey dict. Raises ValueError on wrong secret."""
    salt = base64.b64decode(wrapped["salt"])
    nonce = base64.b64decode(wrapped["nonce"])
    ciphertext = base64.b64decode(wrapped["ciphertext"])
    wrapping_key = derive_key(secret, salt)
    try:
        plaintext = AESGCM(wrapping_key).decrypt(nonce, ciphertext, None)
    except Exception:
        raise ValueError("Wrong PIN or passphrase")
    if len(plaintext) != _KEY_LEN:
        raise ValueError("Unexpected key length after decryption")
    return plaintext


def load_auth_config(auth_json_path: Path) -> dict:
    return json.loads(auth_json_path.read_text())


def save_auth_config(auth_json_path: Path, config: dict) -> None:
    auth_json_path.write_text(json.dumps(config, indent=2))


def is_setup(app_dir: Path) -> bool:
    return (app_dir / "auth.json").exists()


def setup(app_dir: Path, pin: str, passphrase: str) -> str:
    """
    First-time setup. Generates master key, writes auth.json, returns recovery JSON string.
    Raises if already set up.
    """
    auth_path = app_dir / "auth.json"
    if auth_path.exists():
        raise ValueError("Already set up")

    master_key = generate_master_key()
    pin_wrapped = wrap_key(master_key, pin)
    passphrase_wrapped = wrap_key(master_key, passphrase)

    config = {"version": 1, "pin_wrapped": pin_wrapped}
    save_auth_config(auth_path, config)

    recovery = {"version": 1, "passphrase_wrapped": passphrase_wrapped}
    return json.dumps(recovery)


def login(app_dir: Path, pin: str) -> bytes:
    """Unwrap master key with PIN. Returns master_key bytes. Raises ValueError on wrong PIN."""
    config = load_auth_config(app_dir / "auth.json")
    return unwrap_key(config["pin_wrapped"], pin)


def recover(app_dir: Path, recovery_json: str, passphrase: str, new_pin: str) -> bytes:
    """Recover using passphrase, re-wrap with new PIN. Returns master_key bytes."""
    recovery = json.loads(recovery_json)
    master_key = unwrap_key(recovery["passphrase_wrapped"], passphrase)
    config = load_auth_config(app_dir / "auth.json")
    config["pin_wrapped"] = wrap_key(master_key, new_pin)
    save_auth_config(app_dir / "auth.json", config)
    return master_key


def change_pin(app_dir: Path, master_key: bytes, current_pin: str, new_pin: str) -> None:
    """Verify current PIN then re-wrap master key with new PIN."""
    config = load_auth_config(app_dir / "auth.json")
    # Verify current PIN first
    unwrap_key(config["pin_wrapped"], current_pin)
    config["pin_wrapped"] = wrap_key(master_key, new_pin)
    save_auth_config(app_dir / "auth.json", config)
