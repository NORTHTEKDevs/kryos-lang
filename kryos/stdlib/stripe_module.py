"""
Kryos Standard Library - Stripe Module

Registers Stripe payment functions: customers, checkout sessions,
subscriptions, and webhook verification. Uses Stripe REST API directly.
Requires STRIPE_SECRET_KEY env var.
"""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import time
import urllib.parse
import urllib.request
from typing import Any


_STRIPE_API_BASE = "https://api.stripe.com/v1"


def register_stripe_builtins(interpreter) -> None:
    """Register Stripe payment utility functions."""
    env = interpreter.globals

    def _get_api_key() -> str:
        key = os.environ.get("STRIPE_SECRET_KEY", "")
        if not key:
            raise RuntimeError("STRIPE_SECRET_KEY environment variable not set")
        return key

    def _stripe_request(method: str, endpoint: str, data: dict | None = None) -> dict:
        """Make an authenticated request to Stripe API."""
        api_key = _get_api_key()
        url = f"{_STRIPE_API_BASE}/{endpoint}"

        body = None
        if data:
            body = urllib.parse.urlencode(_flatten_dict(data)).encode("utf-8")

        req = urllib.request.Request(url, data=body, method=method)
        req.add_header("Authorization", f"Bearer {api_key}")
        req.add_header("Content-Type", "application/x-www-form-urlencoded")

        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                return json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            error_body = exc.read().decode("utf-8", errors="replace")
            try:
                error_json = json.loads(error_body)
                msg = error_json.get("error", {}).get("message", error_body)
            except Exception:
                msg = error_body
            raise RuntimeError(f"Stripe API error ({exc.code}): {msg}")

    def _flatten_dict(d: dict, prefix: str = "") -> list[tuple[str, str]]:
        """Flatten nested dict for Stripe's form encoding."""
        items: list[tuple[str, str]] = []
        for key, value in d.items():
            full_key = f"{prefix}[{key}]" if prefix else key
            if isinstance(value, dict):
                items.extend(_flatten_dict(value, full_key))
            elif isinstance(value, list):
                for i, v in enumerate(value):
                    if isinstance(v, dict):
                        items.extend(_flatten_dict(v, f"{full_key}[{i}]"))
                    else:
                        items.append((f"{full_key}[{i}]", str(v)))
            else:
                items.append((full_key, str(value)))
        return items

    # ---- Customers ----

    def _stripe_create_customer(email: Any, *args: Any) -> dict:
        """Create a Stripe customer.

        Arguments: email (string), [options_map with name, metadata, etc.]
        Returns: customer object (map)
        """
        data: dict[str, Any] = {"email": str(email)}
        if args and isinstance(args[0], dict):
            for k, v in args[0].items():
                data[k] = v
        return _stripe_request("POST", "customers", data)

    def _stripe_get_customer(customer_id: Any) -> dict:
        """Retrieve a Stripe customer by ID."""
        return _stripe_request("GET", f"customers/{customer_id}")

    # ---- Checkout Sessions ----

    def _stripe_create_checkout(params: Any) -> dict:
        """Create a Stripe Checkout Session.

        Arguments: params_map with:
          - customer (string, optional): customer ID
          - mode: "payment", "subscription", or "setup"
          - success_url: redirect after payment
          - cancel_url: redirect on cancel
          - line_items: array of {price: "price_xxx", quantity: 1}

        Returns: checkout session object with url field

        Usage:
            let session = stripe_create_checkout({
                "mode": "subscription",
                "success_url": "https://myapp.com/success",
                "cancel_url": "https://myapp.com/cancel",
                "line_items": [{"price": "price_xxx", "quantity": 1}]
            })
            // Redirect user to session.url
        """
        if not isinstance(params, dict):
            raise RuntimeError("stripe_create_checkout: argument must be a map")
        return _stripe_request("POST", "checkout/sessions", params)

    # ---- Subscriptions ----

    def _stripe_create_subscription(customer_id: Any, price_id: Any, *args: Any) -> dict:
        """Create a subscription for a customer.

        Arguments: customer_id, price_id, [options_map]
        Returns: subscription object
        """
        data: dict[str, Any] = {
            "customer": str(customer_id),
            "items": [{"price": str(price_id)}],
        }
        if args and isinstance(args[0], dict):
            for k, v in args[0].items():
                data[k] = v
        return _stripe_request("POST", "subscriptions", data)

    def _stripe_cancel_subscription(subscription_id: Any) -> dict:
        """Cancel a subscription."""
        return _stripe_request("DELETE", f"subscriptions/{subscription_id}")

    def _stripe_get_subscription(subscription_id: Any) -> dict:
        """Retrieve a subscription."""
        return _stripe_request("GET", f"subscriptions/{subscription_id}")

    # ---- Payments / Charges ----

    def _stripe_create_payment_intent(amount: Any, currency: Any, *args: Any) -> dict:
        """Create a payment intent.

        Arguments: amount (int, cents), currency (string, e.g. "usd"), [options_map]
        Returns: payment intent object
        """
        data: dict[str, Any] = {
            "amount": str(int(amount)),
            "currency": str(currency),
        }
        if args and isinstance(args[0], dict):
            for k, v in args[0].items():
                data[k] = v
        return _stripe_request("POST", "payment_intents", data)

    # ---- Webhook Verification ----

    def _stripe_verify_webhook(payload: Any, sig_header: Any, secret: Any) -> Any:
        """Verify a Stripe webhook signature.

        Arguments: payload (raw body string), sig_header (Stripe-Signature header), webhook_secret
        Returns: parsed event object if valid, none if invalid
        """
        payload_str = str(payload)
        sig = str(sig_header)
        webhook_secret = str(secret)

        # Parse signature header
        parts: dict[str, str] = {}
        for item in sig.split(","):
            key_val = item.strip().split("=", 1)
            if len(key_val) == 2:
                parts[key_val[0]] = key_val[1]

        timestamp = parts.get("t", "")
        v1_sig = parts.get("v1", "")

        if not timestamp or not v1_sig:
            return None

        # Verify timestamp is recent (within 5 minutes)
        try:
            ts = int(timestamp)
            if abs(time.time() - ts) > 300:
                return None
        except ValueError:
            return None

        # Compute expected signature
        signed_payload = f"{timestamp}.{payload_str}"
        expected = hmac.new(
            webhook_secret.encode("utf-8"),
            signed_payload.encode("utf-8"),
            hashlib.sha256,
        ).hexdigest()

        if not hmac.compare_digest(expected, v1_sig):
            return None

        try:
            return json.loads(payload_str)
        except Exception:
            return None

    # ---- Products & Prices ----

    def _stripe_create_product(name: Any, *args: Any) -> dict:
        """Create a Stripe product."""
        data: dict[str, Any] = {"name": str(name)}
        if args and isinstance(args[0], dict):
            for k, v in args[0].items():
                data[k] = v
        return _stripe_request("POST", "products", data)

    def _stripe_create_price(product_id: Any, amount: Any, currency: Any, *args: Any) -> dict:
        """Create a price for a product.

        Arguments: product_id, amount (cents), currency, [options with recurring interval etc.]
        """
        data: dict[str, Any] = {
            "product": str(product_id),
            "unit_amount": str(int(amount)),
            "currency": str(currency),
        }
        if args and isinstance(args[0], dict):
            for k, v in args[0].items():
                data[k] = v
        return _stripe_request("POST", "prices", data)

    # ---- Portal ----

    def _stripe_create_portal_session(customer_id: Any, return_url: Any) -> dict:
        """Create a billing portal session for self-service management."""
        return _stripe_request("POST", "billing_portal/sessions", {
            "customer": str(customer_id),
            "return_url": str(return_url),
        })

    # ---- Register all ----

    env.define("stripe_create_customer", _stripe_create_customer)
    env.define("stripe_get_customer", _stripe_get_customer)
    env.define("stripe_create_checkout", _stripe_create_checkout)
    env.define("stripe_create_subscription", _stripe_create_subscription)
    env.define("stripe_cancel_subscription", _stripe_cancel_subscription)
    env.define("stripe_get_subscription", _stripe_get_subscription)
    env.define("stripe_create_payment_intent", _stripe_create_payment_intent)
    env.define("stripe_verify_webhook", _stripe_verify_webhook)
    env.define("stripe_create_product", _stripe_create_product)
    env.define("stripe_create_price", _stripe_create_price)
    env.define("stripe_create_portal_session", _stripe_create_portal_session)
