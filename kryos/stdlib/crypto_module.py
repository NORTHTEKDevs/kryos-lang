"""
Kryos Standard Library - Crypto Module

Registers cryptographic utility functions as interpreter built-ins.
Functions: sha256, sha512, md5, hmac_sha256, base64_encode, base64_decode,
hex_encode, hex_decode, random_bytes, uuid.
"""

from __future__ import annotations

import base64 as _base64
import hashlib as _hashlib
import hmac as _hmac
import secrets as _secrets
import uuid as _uuid
from typing import Any


def register_crypto_builtins(interpreter) -> None:
    """Register cryptographic utility functions."""
    env = interpreter.globals

    def _sha256(data: Any) -> str:
        return _hashlib.sha256(str(data).encode("utf-8")).hexdigest()

    def _sha512(data: Any) -> str:
        return _hashlib.sha512(str(data).encode("utf-8")).hexdigest()

    def _md5(data: Any) -> str:
        return _hashlib.md5(str(data).encode("utf-8")).hexdigest()

    def _hmac_sha256(key: Any, data: Any) -> str:
        return _hmac.new(
            str(key).encode("utf-8"),
            str(data).encode("utf-8"),
            _hashlib.sha256,
        ).hexdigest()

    def _base64_encode(data: Any) -> str:
        return _base64.b64encode(str(data).encode("utf-8")).decode("utf-8")

    def _base64_decode(data: Any) -> str:
        try:
            return _base64.b64decode(str(data)).decode("utf-8")
        except Exception as exc:
            raise RuntimeError(f"base64_decode failed: {exc}") from exc

    def _hex_encode(data: Any) -> str:
        return str(data).encode("utf-8").hex()

    def _hex_decode(data: Any) -> str:
        try:
            return bytes.fromhex(str(data)).decode("utf-8")
        except Exception as exc:
            raise RuntimeError(f"hex_decode failed: {exc}") from exc

    def _random_bytes(n: Any) -> str:
        return _secrets.token_hex(int(n))

    def _uuid_fn() -> str:
        return str(_uuid.uuid4())

    env.define("sha256", _sha256)
    env.define("sha512", _sha512)
    env.define("md5", _md5)
    env.define("hmac_sha256", _hmac_sha256)
    env.define("base64_encode", _base64_encode)
    env.define("base64_decode", _base64_decode)
    env.define("hex_encode", _hex_encode)
    env.define("hex_decode", _hex_decode)
    env.define("random_bytes", _random_bytes)
    env.define("uuid", _uuid_fn)
