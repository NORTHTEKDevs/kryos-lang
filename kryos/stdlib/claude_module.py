"""
Kryos Standard Library - Claude Module

Registers Claude/Anthropic API functions for AI-powered applications.
Uses the Anthropic Messages API. Requires ANTHROPIC_API_KEY env var.
No external dependencies — uses urllib.
"""

from __future__ import annotations

import json
import os
import urllib.request
from typing import Any


_ANTHROPIC_API_URL = "https://api.anthropic.com/v1/messages"
_DEFAULT_MODEL = "claude-sonnet-4-20250514"
_DEFAULT_MAX_TOKENS = 4096


def register_claude_builtins(interpreter) -> None:
    """Register Claude/Anthropic API functions."""
    env = interpreter.globals

    def _get_api_key() -> str:
        key = os.environ.get("ANTHROPIC_API_KEY", "")
        if not key:
            raise RuntimeError(
                "claude_message: ANTHROPIC_API_KEY environment variable not set"
            )
        return key

    def _claude_message(prompt: Any, *args: Any) -> str:
        """Send a message to Claude and get a text response.

        Arguments: prompt (string), [options_map]
        Options: model, max_tokens, system, temperature
        Returns: response text (string)

        Usage:
            let answer = claude_message("What is 2+2?")
            let answer = claude_message("Explain X", {"model": "claude-haiku-4-5-20251001", "system": "You are helpful."})
        """
        api_key = _get_api_key()
        prompt_str = str(prompt)

        # Parse options
        opts = args[0] if args and isinstance(args[0], dict) else {}
        model = str(opts.get("model", _DEFAULT_MODEL))
        max_tokens = int(opts.get("max_tokens", _DEFAULT_MAX_TOKENS))
        system = opts.get("system", None)
        temperature = opts.get("temperature", None)

        # Build request body
        body: dict[str, Any] = {
            "model": model,
            "max_tokens": max_tokens,
            "messages": [{"role": "user", "content": prompt_str}],
        }
        if system:
            body["system"] = str(system)
        if temperature is not None:
            body["temperature"] = float(temperature)

        payload = json.dumps(body).encode("utf-8")

        req = urllib.request.Request(
            _ANTHROPIC_API_URL,
            data=payload,
            headers={
                "Content-Type": "application/json",
                "x-api-key": api_key,
                "anthropic-version": "2023-06-01",
            },
            method="POST",
        )

        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                data = json.loads(resp.read().decode("utf-8"))
                # Extract text from content blocks
                content = data.get("content", [])
                texts = [b["text"] for b in content if b.get("type") == "text"]
                return "\n".join(texts)
        except urllib.error.HTTPError as exc:
            error_body = exc.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"claude_message: API error {exc.code}: {error_body[:500]}")
        except Exception as exc:
            raise RuntimeError(f"claude_message: {exc}")

    def _claude_messages(messages: Any, *args: Any) -> str:
        """Send a multi-turn conversation to Claude.

        Arguments: messages (array of maps with role/content), [options_map]
        Returns: response text (string)

        Usage:
            let history = [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi there!"},
                {"role": "user", "content": "What did I say first?"}
            ]
            let answer = claude_messages(history)
        """
        api_key = _get_api_key()

        if not isinstance(messages, list):
            raise RuntimeError("claude_messages: first arg must be array of message maps")

        # Convert message format
        msg_list = []
        for msg in messages:
            if isinstance(msg, dict):
                msg_list.append({
                    "role": str(msg.get("role", "user")),
                    "content": str(msg.get("content", "")),
                })

        opts = args[0] if args and isinstance(args[0], dict) else {}
        model = str(opts.get("model", _DEFAULT_MODEL))
        max_tokens = int(opts.get("max_tokens", _DEFAULT_MAX_TOKENS))
        system = opts.get("system", None)
        temperature = opts.get("temperature", None)

        body: dict[str, Any] = {
            "model": model,
            "max_tokens": max_tokens,
            "messages": msg_list,
        }
        if system:
            body["system"] = str(system)
        if temperature is not None:
            body["temperature"] = float(temperature)

        payload = json.dumps(body).encode("utf-8")

        req = urllib.request.Request(
            _ANTHROPIC_API_URL,
            data=payload,
            headers={
                "Content-Type": "application/json",
                "x-api-key": api_key,
                "anthropic-version": "2023-06-01",
            },
            method="POST",
        )

        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                data = json.loads(resp.read().decode("utf-8"))
                content = data.get("content", [])
                texts = [b["text"] for b in content if b.get("type") == "text"]
                return "\n".join(texts)
        except urllib.error.HTTPError as exc:
            error_body = exc.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"claude_messages: API error {exc.code}: {error_body[:500]}")
        except Exception as exc:
            raise RuntimeError(f"claude_messages: {exc}")

    def _claude_json(prompt: Any, *args: Any) -> Any:
        """Send a message to Claude and parse the response as JSON.

        Arguments: prompt (string), [options_map]
        Returns: parsed JSON (map/array)

        Useful for structured data extraction. Adds JSON instruction to system prompt.
        """
        opts = args[0] if args and isinstance(args[0], dict) else {}
        system = str(opts.get("system", ""))
        json_instruction = "Respond with valid JSON only. No markdown, no explanation, just JSON."
        if system:
            opts["system"] = f"{system}\n{json_instruction}"
        else:
            opts["system"] = json_instruction

        response = _claude_message(prompt, opts)

        # Try to parse JSON from response
        text = response.strip()
        # Strip markdown code fences if present
        if text.startswith("```"):
            lines = text.split("\n")
            text = "\n".join(lines[1:])
            if text.endswith("```"):
                text = text[:-3]
            text = text.strip()

        try:
            return json.loads(text)
        except json.JSONDecodeError:
            raise RuntimeError(f"claude_json: response was not valid JSON: {text[:200]}")

    def _claude_embed(text: Any, *args: Any) -> list:
        """Get an embedding vector for text using Anthropic's API.

        Note: Anthropic doesn't have a native embeddings endpoint yet.
        This is a placeholder that uses a simple hash-based embedding.
        For production, use OpenAI's embedding API or a local model.
        """
        # Placeholder: return a deterministic pseudo-embedding
        import hashlib
        h = hashlib.sha256(str(text).encode()).hexdigest()
        # Convert to 8 float values between -1 and 1
        values = []
        for i in range(0, 64, 8):
            chunk = int(h[i:i+8], 16)
            values.append((chunk / 0xFFFFFFFF) * 2 - 1)
        return values

    # ---- Register all ----

    env.define("claude_message", _claude_message)
    env.define("claude_messages", _claude_messages)
    env.define("claude_json", _claude_json)
    env.define("claude_embed", _claude_embed)
