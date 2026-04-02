"""
Kryos Standard Library - Email Module

Registers email sending functions. Uses Python's smtplib (stdlib).
No external dependencies. Supports SMTP and SMTP+TLS.
"""

from __future__ import annotations

import json
import os
import smtplib
import urllib.parse
import urllib.request
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from typing import Any


def register_email_builtins(interpreter) -> None:
    """Register email utility functions."""
    env = interpreter.globals

    def _send_email(to: Any, subject: Any, body: Any, *args: Any) -> bool:
        """Send an email via SMTP.

        Arguments: to (string or array), subject (string), body (string), [options_map]
        Options:
          from: sender address (default: SMTP_FROM env var)
          html: if true, send as HTML (default: false)
          smtp_host: SMTP server (default: SMTP_HOST env var or localhost)
          smtp_port: SMTP port (default: SMTP_PORT env var or 587)
          smtp_user: SMTP username (default: SMTP_USER env var)
          smtp_pass: SMTP password (default: SMTP_PASS env var)
          reply_to: reply-to address
          cc: CC recipients (string or array)
          bcc: BCC recipients (string or array)

        Returns: true if sent, raises on error
        """
        opts = args[0] if args and isinstance(args[0], dict) else {}

        # Recipients
        to_list = to if isinstance(to, list) else [str(to)]
        cc_list = opts.get("cc", [])
        if isinstance(cc_list, str):
            cc_list = [cc_list]
        bcc_list = opts.get("bcc", [])
        if isinstance(bcc_list, str):
            bcc_list = [bcc_list]

        all_recipients = to_list + cc_list + bcc_list

        # Config
        from_addr = str(opts.get("from", os.environ.get("SMTP_FROM", "noreply@localhost")))
        smtp_host = str(opts.get("smtp_host", os.environ.get("SMTP_HOST", "localhost")))
        smtp_port = int(opts.get("smtp_port", os.environ.get("SMTP_PORT", "587")))
        smtp_user = opts.get("smtp_user", os.environ.get("SMTP_USER", ""))
        smtp_pass = opts.get("smtp_pass", os.environ.get("SMTP_PASS", ""))
        is_html = bool(opts.get("html", False))

        # Sanitize headers to prevent CRLF injection
        def _sanitize_header(val: str) -> str:
            return val.replace("\r", "").replace("\n", "")

        # Build message
        msg = MIMEMultipart("alternative")
        msg["From"] = _sanitize_header(from_addr)
        msg["To"] = _sanitize_header(", ".join(to_list))
        msg["Subject"] = _sanitize_header(str(subject))

        if cc_list:
            msg["Cc"] = _sanitize_header(", ".join(cc_list))
        if opts.get("reply_to"):
            msg["Reply-To"] = _sanitize_header(str(opts["reply_to"]))

        content_type = "html" if is_html else "plain"
        msg.attach(MIMEText(str(body), content_type, "utf-8"))

        try:
            if smtp_port == 465:
                server = smtplib.SMTP_SSL(smtp_host, smtp_port, timeout=30)
            else:
                server = smtplib.SMTP(smtp_host, smtp_port, timeout=30)
                server.starttls()

            if smtp_user and smtp_pass:
                server.login(str(smtp_user), str(smtp_pass))

            server.sendmail(from_addr, all_recipients, msg.as_string())
            server.quit()
            return True
        except Exception as exc:
            raise RuntimeError(f"send_email: {exc}")

    def _send_email_resend(to: Any, subject: Any, body: Any, *args: Any) -> dict:
        """Send email via Resend API (https://resend.com).

        Arguments: to (string or array), subject, body, [options_map]
        Options: from, html (bool), reply_to, api_key (or RESEND_API_KEY env)
        Returns: response map with id field
        """
        opts = args[0] if args and isinstance(args[0], dict) else {}
        api_key = str(opts.get("api_key", os.environ.get("RESEND_API_KEY", "")))
        if not api_key:
            raise RuntimeError("send_email_resend: RESEND_API_KEY not set")

        to_list = to if isinstance(to, list) else [str(to)]
        from_addr = str(opts.get("from", os.environ.get("RESEND_FROM", "onboarding@resend.dev")))
        is_html = bool(opts.get("html", False))

        payload = {
            "from": from_addr,
            "to": to_list,
            "subject": str(subject),
        }
        if is_html:
            payload["html"] = str(body)
        else:
            payload["text"] = str(body)
        if opts.get("reply_to"):
            payload["reply_to"] = str(opts["reply_to"])

        data = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(
            "https://api.resend.com/emails",
            data=data,
            headers={
                "Authorization": f"Bearer {api_key}",
                "Content-Type": "application/json",
            },
            method="POST",
        )

        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                return json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            error_body = exc.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"send_email_resend: API error {exc.code}: {error_body}")
        except Exception as exc:
            raise RuntimeError(f"send_email_resend: {exc}")

    # ---- Register ----
    env.define("send_email", _send_email)
    env.define("send_email_resend", _send_email_resend)
