"""
Kryos Standard Library - Auth Module

Registers authentication utilities: JWT creation/verification and
password hashing. Uses HMAC-SHA256 for JWT and PBKDF2 for passwords.
No external dependencies.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import os
import time
from typing import Any


def register_auth_builtins(interpreter) -> None:
    """Register authentication utility functions."""
    env = interpreter.globals

    # ---- JWT ----

    def _jwt_sign(payload: Any, secret: Any, *args: Any) -> str:
        """Create a signed JWT token.

        Arguments: payload (map), secret (string), [expires_in_seconds]
        Returns: JWT token string
        """
        if not isinstance(payload, dict):
            raise RuntimeError("jwt_sign: payload must be a map")

        secret_str = str(secret)

        # Add standard claims
        claims = dict(payload)
        claims["iat"] = int(time.time())
        if args:
            expires_in = int(args[0])
            claims["exp"] = claims["iat"] + expires_in

        # Header
        header = {"alg": "HS256", "typ": "JWT"}

        # Encode
        header_b64 = _b64url_encode(json.dumps(header, separators=(",", ":")))
        payload_b64 = _b64url_encode(json.dumps(claims, separators=(",", ":")))

        # Sign
        signing_input = f"{header_b64}.{payload_b64}"
        signature = hmac.new(
            secret_str.encode("utf-8"),
            signing_input.encode("utf-8"),
            hashlib.sha256,
        ).digest()
        signature_b64 = _b64url_encode_bytes(signature)

        return f"{header_b64}.{payload_b64}.{signature_b64}"

    def _jwt_verify(token: Any, secret: Any) -> Any:
        """Verify and decode a JWT token.

        Arguments: token (string), secret (string)
        Returns: payload map if valid, none if invalid/expired
        """
        token_str = str(token)
        secret_str = str(secret)

        parts = token_str.split(".")
        if len(parts) != 3:
            return None

        header_b64, payload_b64, signature_b64 = parts

        # Verify signature
        signing_input = f"{header_b64}.{payload_b64}"
        expected = hmac.new(
            secret_str.encode("utf-8"),
            signing_input.encode("utf-8"),
            hashlib.sha256,
        ).digest()

        try:
            actual = _b64url_decode_bytes(signature_b64)
        except Exception:
            return None

        if not hmac.compare_digest(expected, actual):
            return None

        # Decode payload
        try:
            payload = json.loads(_b64url_decode(payload_b64))
        except Exception:
            return None

        # Check expiration
        if "exp" in payload and payload["exp"] < time.time():
            return None

        return payload

    def _jwt_decode(token: Any) -> Any:
        """Decode a JWT WITHOUT verifying. For inspection only.

        Arguments: token (string)
        Returns: payload map or none
        """
        parts = str(token).split(".")
        if len(parts) != 3:
            return None
        try:
            return json.loads(_b64url_decode(parts[1]))
        except Exception:
            return None

    # ---- Password Hashing (PBKDF2) ----

    def _hash_password(password: Any, *args: Any) -> str:
        """Hash a password using PBKDF2-SHA256.

        Arguments: password (string), [iterations=600000]
        Returns: encoded hash string (format: pbkdf2:iterations:salt:hash)
        """
        pwd = str(password)
        iterations = int(args[0]) if args else 600000
        salt = os.urandom(32)
        dk = hashlib.pbkdf2_hmac("sha256", pwd.encode("utf-8"), salt, iterations)
        salt_hex = salt.hex()
        hash_hex = dk.hex()
        return f"pbkdf2:{iterations}:{salt_hex}:{hash_hex}"

    def _verify_password(password: Any, hashed: Any) -> bool:
        """Verify a password against a PBKDF2 hash.

        Arguments: password (string), hashed (string from hash_password)
        Returns: true if matches, false otherwise
        """
        pwd = str(password)
        hash_str = str(hashed)

        parts = hash_str.split(":")
        if len(parts) != 4 or parts[0] != "pbkdf2":
            return False

        iterations = int(parts[1])
        salt = bytes.fromhex(parts[2])
        expected = bytes.fromhex(parts[3])

        dk = hashlib.pbkdf2_hmac("sha256", pwd.encode("utf-8"), salt, iterations)
        return hmac.compare_digest(dk, expected)

    # ---- Helpers ----

    def _b64url_encode(data: str) -> str:
        return base64.urlsafe_b64encode(data.encode("utf-8")).rstrip(b"=").decode("ascii")

    def _b64url_encode_bytes(data: bytes) -> str:
        return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")

    def _b64url_decode(data: str) -> str:
        padded = data + "=" * (4 - len(data) % 4)
        return base64.urlsafe_b64decode(padded).decode("utf-8")

    def _b64url_decode_bytes(data: str) -> bytes:
        padded = data + "=" * (4 - len(data) % 4)
        return base64.urlsafe_b64decode(padded)

    # ---- Token generation utilities ----

    def _generate_token(*args: Any) -> str:
        """Generate a cryptographically secure random token.

        Arguments: [length=32]
        Returns: hex-encoded random token
        """
        length = int(args[0]) if args else 32
        return os.urandom(length).hex()

    # ---- Register all ----

    env.define("jwt_sign", _jwt_sign)
    env.define("jwt_verify", _jwt_verify)
    env.define("jwt_decode", _jwt_decode)
    env.define("hash_password", _hash_password)
    env.define("verify_password", _verify_password)
    env.define("generate_token", _generate_token)
