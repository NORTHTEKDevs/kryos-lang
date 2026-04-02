"""
Kryos Standard Library - Config Module

Registers configuration management functions: .env file loading,
environment variable access with defaults and requirements.

Note: env_get and env_set are defined in io_module.py.
This module adds env_load, env_require, env_default, env_all, and config.
"""

from __future__ import annotations

import os
from typing import Any


def register_config_builtins(interpreter) -> None:
    """Register configuration utility functions."""
    env = interpreter.globals

    def _env_load(*args: Any) -> int:
        """Load environment variables from a .env file.

        Arguments: [path] (default: ".env")
        Returns: number of variables loaded

        .env format:
            KEY=value
            KEY="quoted value"
            # comments are ignored
            export KEY=value  (export prefix is stripped)
        """
        path = str(args[0]) if args else ".env"
        # Security: restrict to project-relative paths (no absolute or traversal)
        if os.path.isabs(path) or ".." in path.split(os.sep):
            raise RuntimeError(f"env_load: path must be relative and within project: {path}")
        if not os.path.isfile(path):
            return 0

        count = 0
        with open(path, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#"):
                    continue
                if line.startswith("export "):
                    line = line[7:].strip()

                if "=" not in line:
                    continue

                key, _, value = line.partition("=")
                key = key.strip()
                value = value.strip()

                # Strip quotes
                if len(value) >= 2 and value[0] == value[-1] and value[0] in ('"', "'"):
                    value = value[1:-1]

                os.environ[key] = value
                count += 1
        return count

    def _env_require(key: Any) -> str:
        """Get a required environment variable. Raises if not set.

        Arguments: key (string)
        Returns: value (string)
        """
        k = str(key)
        value = os.environ.get(k)
        if value is None:
            raise RuntimeError(f"env_require: required environment variable '{k}' is not set")
        return value

    def _env_get_default(key: Any, default: Any) -> str:
        """Get an environment variable with a default.

        Arguments: key (string), default (string)
        Returns: value or default
        """
        return os.environ.get(str(key), str(default))

    def _env_all() -> dict:
        """Get all environment variables as a map."""
        return dict(os.environ)

    def _config(defaults: Any) -> dict:
        """Create a config map from environment variables with defaults.

        Arguments: defaults_map (keys are env var names, values are defaults)
        Returns: config map with resolved values

        Usage:
            let cfg = config({
                "PORT": "8080",
                "DATABASE_URL": "sqlite://:memory:",
                "JWT_SECRET": ""
            })
            // cfg.PORT will be env var PORT or "8080"
        """
        if not isinstance(defaults, dict):
            raise RuntimeError("config: argument must be a map of defaults")
        result = {}
        for key, default in defaults.items():
            result[str(key)] = os.environ.get(str(key), str(default))
        return result

    # ---- Register ----
    env.define("env_load", _env_load)
    env.define("env_require", _env_require)
    env.define("env_default", _env_get_default)
    env.define("env_all", _env_all)
    env.define("config", _config)
