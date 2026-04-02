"""
Kryos Standard Library - DateTime Module

Date, time, and timestamp functions wrapping Python's datetime and time modules.
"""
from __future__ import annotations
import time as _time
from datetime import datetime, timezone, timedelta
from typing import Any

def register_datetime_builtins(interpreter) -> None:
    env = interpreter.globals

    def _now() -> float:
        """Current Unix timestamp in seconds (float)."""
        return _time.time()

    def _now_ms() -> int:
        """Current Unix timestamp in milliseconds (int)."""
        return int(_time.time() * 1000)

    def _now_iso() -> str:
        """Current time as ISO 8601 string (UTC)."""
        return datetime.now(timezone.utc).isoformat()

    def _now_local() -> str:
        """Current local time as ISO 8601 string."""
        return datetime.now().isoformat()

    def _timestamp_to_iso(ts: Any) -> str:
        """Convert Unix timestamp to ISO 8601 string (UTC)."""
        return datetime.fromtimestamp(float(ts), tz=timezone.utc).isoformat()

    def _iso_to_timestamp(iso_str: Any) -> float:
        """Convert ISO 8601 string to Unix timestamp."""
        dt = datetime.fromisoformat(str(iso_str).replace("Z", "+00:00"))
        return dt.timestamp()

    def _format_date(ts: Any, fmt: Any) -> str:
        """Format a Unix timestamp with strftime format string.

        Common formats: %Y-%m-%d, %H:%M:%S, %Y-%m-%d %H:%M:%S
        """
        return datetime.fromtimestamp(float(ts), tz=timezone.utc).strftime(str(fmt))

    def _parse_date(date_str: Any, fmt: Any) -> float:
        """Parse a date string with strftime format. Returns Unix timestamp."""
        dt = datetime.strptime(str(date_str), str(fmt))
        return dt.timestamp()

    def _date_add(ts: Any, seconds: Any) -> float:
        """Add seconds to a timestamp. Use negative for subtraction."""
        return float(ts) + float(seconds)

    def _date_diff(ts1: Any, ts2: Any) -> float:
        """Difference between two timestamps in seconds."""
        return float(ts1) - float(ts2)

    def _date_parts(ts: Any) -> dict:
        """Break a timestamp into year, month, day, hour, minute, second."""
        dt = datetime.fromtimestamp(float(ts), tz=timezone.utc)
        return {
            "year": dt.year, "month": dt.month, "day": dt.day,
            "hour": dt.hour, "minute": dt.minute, "second": dt.second,
            "weekday": dt.weekday(), "day_of_year": dt.timetuple().tm_yday,
        }

    def _duration(value: Any, unit: Any) -> float:
        """Convert a duration to seconds. Units: s, m, h, d, w."""
        v = float(value)
        u = str(unit).lower()
        multipliers = {"s": 1, "m": 60, "h": 3600, "d": 86400, "w": 604800}
        if u not in multipliers:
            raise RuntimeError(f"duration: unknown unit '{u}'. Use s, m, h, d, w")
        return v * multipliers[u]

    env.define("now", _now)
    env.define("now_ms", _now_ms)
    env.define("now_iso", _now_iso)
    env.define("now_local", _now_local)
    env.define("timestamp_to_iso", _timestamp_to_iso)
    env.define("iso_to_timestamp", _iso_to_timestamp)
    env.define("format_date", _format_date)
    env.define("parse_date", _parse_date)
    env.define("date_add", _date_add)
    env.define("date_diff", _date_diff)
    env.define("date_parts", _date_parts)
    env.define("duration", _duration)
