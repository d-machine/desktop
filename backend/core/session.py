import secrets
import threading

_sessions: dict[str, bytes] = {}  # token → master_key
_lock = threading.Lock()


def create_session(master_key: bytes) -> str:
    token = secrets.token_hex(32)
    with _lock:
        # Only one session at a time for a single-user local app.
        _sessions.clear()
        _sessions[token] = master_key
    return token


def get_key(token: str) -> bytes:
    with _lock:
        key = _sessions.get(token)
    if key is None:
        raise KeyError("Invalid or expired session token")
    return key


def has_active_session() -> bool:
    with _lock:
        return bool(_sessions)


def get_active_key() -> bytes | None:
    with _lock:
        if not _sessions:
            return None
        return next(iter(_sessions.values()))


def destroy_session(token: str) -> None:
    with _lock:
        _sessions.pop(token, None)


def destroy_all() -> None:
    with _lock:
        _sessions.clear()
