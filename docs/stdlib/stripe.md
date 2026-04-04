# std::stripe

Stripe payment integration using the REST API directly. No external SDK required. Requires the `STRIPE_SECRET_KEY` environment variable.

```kryos
import std::stripe
```

---

### stripe_create_customer

`stripe_create_customer(email: String) -> Map`
`stripe_create_customer(email: String, options: Map) -> Map`

Create a Stripe customer. Returns the full customer object from the API.

**Options map:** Any valid Stripe customer field -- `name`, `metadata`, `phone`, `description`, etc.

**Example:**
```kryos
let customer = stripe_create_customer("alice@example.com")
print(customer.id)  // cus_abc123
```

```kryos
let customer = stripe_create_customer("alice@example.com", map_from(
    "name", "Alice Smith",
    "metadata", map_from("plan", "pro", "source", "landing-page")
))
```

**See also:** stripe_get_customer

---

### stripe_get_customer

`stripe_get_customer(customer_id: String) -> Map`

Retrieve a Stripe customer by ID.

**Example:**
```kryos
let customer = stripe_get_customer("cus_abc123")
print(customer.email)  // alice@example.com
```

**See also:** stripe_create_customer

---

### stripe_create_checkout

`stripe_create_checkout(params: Map) -> Map`

Create a Stripe Checkout Session. Returns the session object, including a `url` field to redirect the user to.

**Required params:**
| Key | Description |
|-----|-------------|
| `mode` | `"payment"`, `"subscription"`, or `"setup"` |
| `success_url` | Redirect URL after successful payment |
| `cancel_url` | Redirect URL on cancellation |
| `line_items` | Array of `{price: "price_xxx", quantity: 1}` maps |

**Optional params:** `customer` (existing customer ID), `metadata`, etc.

**Example:**
```kryos
let session = stripe_create_checkout(map_from(
    "mode", "subscription",
    "success_url", "https://myapp.com/success",
    "cancel_url", "https://myapp.com/cancel",
    "line_items", [map_from("price", "price_xxx", "quantity", 1)]
))
print(session.url)  // https://checkout.stripe.com/...
```

**Edge cases:**
- Raises if the argument is not a map.
- Raises on Stripe API errors (invalid price ID, missing fields, etc.).

---

### stripe_create_subscription

`stripe_create_subscription(customer_id: String, price_id: String) -> Map`
`stripe_create_subscription(customer_id: String, price_id: String, options: Map) -> Map`

Create a subscription for an existing customer. Returns the subscription object.

**Example:**
```kryos
let sub = stripe_create_subscription("cus_abc123", "price_monthly_pro")
print(sub.id)      // sub_xyz789
print(sub.status)  // active
```

```kryos
// With trial period
let sub = stripe_create_subscription("cus_abc123", "price_monthly_pro", map_from(
    "trial_period_days", "14"
))
```

**See also:** stripe_get_subscription, stripe_cancel_subscription

---

### stripe_get_subscription

`stripe_get_subscription(subscription_id: String) -> Map`

Retrieve a subscription by ID.

**Example:**
```kryos
let sub = stripe_get_subscription("sub_xyz789")
print(sub.status)           // active
print(sub.current_period_end)  // 1714000000
```

**See also:** stripe_cancel_subscription

---

### stripe_cancel_subscription

`stripe_cancel_subscription(subscription_id: String) -> Map`

Cancel a subscription immediately. Returns the cancelled subscription object.

**Example:**
```kryos
let sub = stripe_cancel_subscription("sub_xyz789")
print(sub.status)  // canceled
```

**See also:** stripe_get_subscription

---

### stripe_create_payment_intent

`stripe_create_payment_intent(amount: Int, currency: String) -> Map`
`stripe_create_payment_intent(amount: Int, currency: String, options: Map) -> Map`

Create a payment intent for one-time charges. Amount is in cents.

**Example:**
```kryos
// Charge $49.99
let intent = stripe_create_payment_intent(4999, "usd")
print(intent.id)             // pi_abc123
print(intent.client_secret)  // pi_abc123_secret_xyz
```

```kryos
// With customer and metadata
let intent = stripe_create_payment_intent(4999, "usd", map_from(
    "customer", "cus_abc123",
    "metadata", map_from("order_id", "ord_456")
))
```

**Edge cases:**
- Amount is in the smallest currency unit (cents for USD, pence for GBP, etc.).

---

### stripe_verify_webhook

`stripe_verify_webhook(payload: String, sig_header: String, secret: String) -> Map | Nil`

Verify a Stripe webhook signature. Returns the parsed event object if valid, `nil` if the signature is invalid or the timestamp is stale.

**Example:**
```kryos
let event = stripe_verify_webhook(raw_body, sig_header, env_require("STRIPE_WEBHOOK_SECRET"))
if event == nil {
    print("Invalid webhook signature")
    exit(1)
}

if event.type == "checkout.session.completed" {
    let session = event.data.object
    print("Payment from: " + session.customer_email)
}
```

**Edge cases:**
- Returns `nil` if the timestamp is more than 5 minutes old (replay protection).
- Uses constant-time comparison for the signature.

---

### stripe_create_product

`stripe_create_product(name: String) -> Map`
`stripe_create_product(name: String, options: Map) -> Map`

Create a Stripe product.

**Example:**
```kryos
let product = stripe_create_product("Pro Plan", map_from(
    "description", "Full access to all features"
))
print(product.id)  // prod_abc123
```

**See also:** stripe_create_price

---

### stripe_create_price

`stripe_create_price(product_id: String, amount: Int, currency: String) -> Map`
`stripe_create_price(product_id: String, amount: Int, currency: String, options: Map) -> Map`

Create a price for a product. Amount is in cents.

**Example:**
```kryos
// One-time price
let price = stripe_create_price("prod_abc123", 4999, "usd")

// Recurring monthly price
let price = stripe_create_price("prod_abc123", 999, "usd", map_from(
    "recurring", map_from("interval", "month")
))
print(price.id)  // price_xyz789
```

**See also:** stripe_create_product, stripe_create_subscription

---

### stripe_create_portal_session

`stripe_create_portal_session(customer_id: String, return_url: String) -> Map`

Create a billing portal session for customer self-service (update payment method, cancel, view invoices).

**Example:**
```kryos
let portal = stripe_create_portal_session("cus_abc123", "https://myapp.com/settings")
print(portal.url)  // https://billing.stripe.com/session/...
// Redirect user to portal.url
```
