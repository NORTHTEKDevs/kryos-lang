# std::email

Email sending via SMTP and the Resend API. No external dependencies -- uses Python's built-in `smtplib` for SMTP and `urllib` for Resend.

```kryos
import std::email
```

---

### send_email

`send_email(to: String | Array, subject: String, body: String) -> Bool`
`send_email(to: String | Array, subject: String, body: String, options: Map) -> Bool`

Send an email via SMTP. Returns `true` on success, raises on error.

**Options map:**
| Key | Default | Description |
|-----|---------|-------------|
| `from` | `SMTP_FROM` env var or `noreply@localhost` | Sender address |
| `html` | `false` | Send body as HTML |
| `smtp_host` | `SMTP_HOST` env var or `localhost` | SMTP server hostname |
| `smtp_port` | `SMTP_PORT` env var or `587` | SMTP port (587 for STARTTLS, 465 for SSL) |
| `smtp_user` | `SMTP_USER` env var | SMTP auth username |
| `smtp_pass` | `SMTP_PASS` env var | SMTP auth password |
| `reply_to` | none | Reply-to address |
| `cc` | none | CC recipients (string or array) |
| `bcc` | none | BCC recipients (string or array) |

**Example:**
```kryos
send_email("user@example.com", "Welcome", "Thanks for signing up!")
```

```kryos
send_email(
    ["alice@co.com", "bob@co.com"],
    "Weekly Report",
    "<h1>Report</h1><p>All systems operational.</p>",
    map_from(
        "html", true,
        "from", "reports@myapp.com",
        "smtp_host", "smtp.gmail.com",
        "smtp_user", "reports@myapp.com",
        "smtp_pass", env_get("GMAIL_APP_PASSWORD")
    )
)
```

**Edge cases:**
- Port 465 uses direct SSL connection. All other ports use STARTTLS.
- SMTP credentials are optional (some local servers accept unauthenticated mail).
- Headers are sanitized against CRLF injection.
- Connection timeout is 30 seconds.

**See also:** send_email_resend

---

### send_email_resend

`send_email_resend(to: String | Array, subject: String, body: String) -> Map`
`send_email_resend(to: String | Array, subject: String, body: String, options: Map) -> Map`

Send an email via the Resend API (https://resend.com). Returns the API response map (includes an `id` field).

Requires `RESEND_API_KEY` environment variable or `api_key` in the options map.

**Options map:**
| Key | Default | Description |
|-----|---------|-------------|
| `api_key` | `RESEND_API_KEY` env var | Resend API key |
| `from` | `RESEND_FROM` env var or `onboarding@resend.dev` | Sender address |
| `html` | `false` | Send body as HTML |
| `reply_to` | none | Reply-to address |

**Example:**
```kryos
env_load()  // load RESEND_API_KEY from .env

let result = send_email_resend(
    "user@example.com",
    "Verify your email",
    "<a href='https://myapp.com/verify?t=abc'>Click here</a>",
    map_from("html", true, "from", "hello@myapp.com")
)
print(result.id)  // resend message ID
```

**Edge cases:**
- Raises if `RESEND_API_KEY` is not set and no `api_key` option is provided.
- Raises on HTTP errors from the Resend API (includes the error body in the message).

**See also:** send_email
