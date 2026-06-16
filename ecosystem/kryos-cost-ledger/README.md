# kryos-cost-ledger

An append-only, hash-chained spend ledger for Kryos agents.

`kryos-agent-loop` accumulates `ComputeCost` across turns.
`kryos-bench-governed` enforces per-call budgets at runtime.
The missing piece is a *persistent, tamper-evident record* of what was spent
— an artifact you can hand to a billing dispute. This package provides that.

Every `ledger_append(ledger, cost)` call extends a hash chain: the new entry's
hash covers all its cost fields plus the preceding entry's hash. Any post-hoc
mutation (altered token count, spliced entry, corrupted hash) is detectable by
`ledger_verify`, which reports the first broken link.

## How it works

Each `LedgerEntry` carries:

| Field | Type | Description |
|-------|------|-------------|
| `seq` | `i64` | Monotone sequence number (0-based) |
| `tokens_used` | `i64` | Tokens from `ComputeCost.tokens_used` |
| `api_calls` | `i64` | API calls from `ComputeCost.api_calls` |
| `wall_ms` | `i64` | Wall time from `ComputeCost.wall_time_ms` (truncated to ms) |
| `prev_hash` | `str` | Hash of the preceding entry (genesis string for entry 0) |
| `hash` | `str` | `entry_hash(seq|tokens|calls|wall_ms|prev_hash)` |

`ledger_verify(ledger)` walks the chain and returns:
- `-1` if every entry's `prev_hash` and self-hash are correct (clean)
- The `seq` of the first broken link (tampered entry detected at that index)

```
kryos test --path ecosystem/kryos-cost-ledger
```

## API

```
// Create an empty ledger
fn ledger_new() -> [LedgerEntry]

// Append one cost event; returns the updated ledger
fn ledger_append(ledger: [LedgerEntry], cost: ComputeCost) -> [LedgerEntry]

// Bridge alias: record from any source (BudgetedChat, CostTracker, etc.)
fn record_cost(ledger: [LedgerEntry], cost: ComputeCost) -> [LedgerEntry]

// Verify the chain; -1 = clean, N = first broken seq
fn ledger_verify(ledger: [LedgerEntry]) -> i64

// Serialize as JSON lines (one object per line)
fn ledger_to_jsonlines(ledger: [LedgerEntry]) -> str

// Persist to disk  @capabilities(io)
fn ledger_save(path: str, ledger: [LedgerEntry])
```

## Hash algorithm note

The spec asks for `sha256` but `std::crypto::sha256` uses `ptr_byte_at`, a
low-level builtin not available in the Cranelift JIT backend (`kryos test`).
This package uses `entry_hash` — a pure-Kryos polynomial hash (djb2-style,
mod 10^9+7) that works everywhere. **This is tamper-EVIDENT, not
cryptographically secure.** Upgrading to sha256 once the JIT supports it
requires only a one-line swap in `_entry_hash`.

`usd` and `energy_kwh` fields from `ComputeCost` are always 0.0 per spec and
are not tracked (spec: "usd/energy ComputeCost fields are 0.0; ledger tracks
tokens/calls/wall only").

## Layout

```
kryos.toml           package manifest
src/ledger.kry       LedgerEntry, ledger_new, ledger_append, ledger_verify,
                     ledger_to_jsonlines, ledger_save, record_cost, entry_hash
tests/test_ledger.kry  9 @test functions
demo_ledger.kry      end-to-end walkthrough (no network required)
```

## Run it

```
kryos test --path ecosystem/kryos-cost-ledger
kryos run  ecosystem/kryos-cost-ledger/demo_ledger.kry
```

## Design constraints

- **Single-writer only.** Concurrent appends would require a lock or CAS;
  neither is in scope for this MVP.
- **Tamper-evident, not tamper-proof.** An adversary who can rewrite the entire
  file including all hashes can construct a valid-looking alternate ledger.
  The README must not overclaim (per spec).
- **No signatures.** Signing is listed as an out-of-scope extension.
- **No merkle proofs or multi-writer support.** Both deferred.

## License

Apache-2.0. See `LICENSE`.
