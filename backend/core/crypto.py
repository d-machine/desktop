"""
RSA keypair management and hybrid-decryption for /persons/secure API responses.

The server encrypts responses with RSA-OAEP + AES-256-GCM.
Wire format (dot-separated base64): <enc_aes_key>.<nonce>.<ciphertext+tag>
"""
import base64
import sqlite3

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import padding, rsa
from cryptography.hazmat.primitives.ciphers.aead import AESGCM


def _get_setting(conn: sqlite3.Connection, key: str) -> str:
    row = conn.execute("SELECT value FROM app_settings WHERE key=?", (key,)).fetchone()
    return row[0] if row else ""


def _set_setting(conn: sqlite3.Connection, key: str, value: str) -> None:
    conn.execute(
        "INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)", (key, value)
    )
    conn.commit()


def ensure_keypair(conn: sqlite3.Connection) -> str:
    """
    Return the base64-encoded public key PEM, generating the RSA keypair on first call.
    The private key is stored in app_settings (already AES-256-GCM encrypted at rest).
    """
    pub_b64 = _get_setting(conn, "rsa_public_key_b64")
    if pub_b64:
        return pub_b64

    private_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)

    priv_pem = private_key.private_bytes(
        serialization.Encoding.PEM,
        serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption(),
    )
    pub_pem = private_key.public_key().public_bytes(
        serialization.Encoding.PEM,
        serialization.PublicFormat.SubjectPublicKeyInfo,
    )
    pub_b64 = base64.b64encode(pub_pem).decode()

    _set_setting(conn, "rsa_private_key_pem", priv_pem.decode())
    _set_setting(conn, "rsa_public_key_b64", pub_b64)
    return pub_b64


def decrypt_server_response(conn: sqlite3.Connection, encrypted: str) -> str:
    """
    Decrypt a dot-separated base64 payload from the server.
    Returns the decrypted plaintext string.
    Raises ValueError on any decryption failure.
    """
    priv_pem = _get_setting(conn, "rsa_private_key_pem")
    if not priv_pem:
        raise ValueError("No RSA private key stored — call ensure_keypair() first")

    try:
        private_key = serialization.load_pem_private_key(priv_pem.encode(), password=None)
        enc_aes_b64, nonce_b64, ct_b64 = encrypted.split(".")
        aes_key = private_key.decrypt(
            base64.b64decode(enc_aes_b64),
            padding.OAEP(
                mgf=padding.MGF1(algorithm=hashes.SHA256()),
                algorithm=hashes.SHA256(),
                label=None,
            ),
        )
        plaintext = AESGCM(aes_key).decrypt(
            base64.b64decode(nonce_b64),
            base64.b64decode(ct_b64),
            None,
        )
        return plaintext.decode()
    except Exception as exc:
        raise ValueError(f"Decryption failed: {exc}") from exc
