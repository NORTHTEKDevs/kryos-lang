# kryos-embed

Deploy capability-gated private AI agents inside existing software -- CRMs, ERPs,
internal tools -- in any language. Data never leaves the process. The compiler
verifies the agent's authority, and the host refuses over-privileged agents before
loading. Budget refusal fires before spend. Every answer carries provenance.

## The problem this solves

Adding an AI assistant to existing software usually means:

1. Sending user data to an external API endpoint.
2. Hoping the model only uses the data you intended.
3. No compile-time guarantee about what the model is authorized to do.
4. No per-call spend cap enforced at the call site.

kryos-embed addresses all four. The agent runs inside the host process as a
compiled Kryos module (native DLL or WASM binary). Capability declarations are
verified by the Kryos compiler at build time and re-checked by the host at load
time. Budget enforcement and provenance tagging happen inside the agent before
any LLM call.

## How it works

```
                            ┌─────────────────────────────────────────┐
                            │  Host process (Python / Go / Node / C#)  │
                            │                                           │
  agent.caps.json ──────> capability gate ──→ BLOCKED if over-privileged
                            │                                           │
  (gate passes)             │                                           │
                            ↓                                           │
                    kryos_embed_agent.dll  (or .wasm)                   │
                    ┌───────────────────┐                               │
  request JSON ──→  │  budget gate      │ ──→ refuse + 0 spend if over  │
  {question, budget}│  mock-LLM stub    │ ──→ answer + source tag       │
                    │  provenance tag   │                               │
                    └───────────────────┘                               │
                            │                                           │
  response JSON ←───────────┘                                           │
  {answered, answer, source, spend_cents, reason}                       │
                            └─────────────────────────────────────────┘
```

### Capability gate (before DLL load)

Every agent ships with `agent.caps.json`. The host reads this file and compares
each export's declared capabilities against its own allow-set. If any export
claims a capability the host did not grant, the DLL is never loaded and a
`CapabilityViolation` error is returned immediately.

```json
{
  "exports": {
    "agent_call": {
      "capabilities": ["ffi"],
      "verified": true
    }
  }
}
```

`"verified": true` is honest: it is only written by `build.sh` after the Kryos
compiler accepted the source under `--strict-capabilities`. A build that fails
the compiler check never writes the manifest.

### Budget gate (inside the agent, before LLM)

The host passes `budget_cents` in the request JSON. The agent compares it against
the configured cost per call before invoking the LLM stub. A refused call returns
`answered=0` and `spend_cents=0`. No charge is recorded.

### Provenance

Every answered response includes `"source": "<backend-tag>"`. In the current
mock backend the tag is `"mock-llm-v1"`. A real deployment replaces this with
the actual model identifier from the LLM provider's response metadata.

### WASM vs DLL

| Transport | Sandbox strength | Tested on |
|-----------|-----------------|-----------|
| Native DLL (Windows x86-64) | Process isolation only; any Windows API callable in principle | Python, Go, C# (recipe) |
| WASM (Node host) | Module-level sandbox; every host import is enumerated at load; file/TCP/process access is physically impossible without an explicit host export | Node |

The WASM story is stronger for untrusted agents: the import manifest printed at
load time is the complete list of host functions the module can call. Anything
absent from that list cannot be called regardless of what the Kryos source does.

## Quickstart

### Prerequisites

- Built DLL and manifest (from repo root):

  ```
  bash ecosystem/kryos-embed/build.sh
  ```

  This runs the compiler's capability check, links the DLL, and writes
  `dist/agent.caps.json`.

- For Node/WASM, the node host's check.sh also builds the WASM binary.

### Python

```python
from kryos_agent import KryosAgent, CapabilityViolation

agent = KryosAgent(
    dll_path="dist/kryos_embed_agent.dll",
    caps_path="dist/agent.caps.json",
    allowed_caps=["ffi"],        # net:http, net:tcp, fs:write -- not granted
)

result = agent.ask("Which accounts are overdue?", budget_cents=5)
print(result["answered"], result["source"], result["spend_cents"])
# -> 1  'mock-llm-v1'  3
```

Capability violation (DLL never loads):

```python
try:
    KryosAgent(..., allowed_caps=[])   # ffi not granted
except CapabilityViolation as e:
    print(e)   # export 'agent_call' requires capability 'ffi' ...
```

Over-budget refusal (no spend):

```python
result = agent.ask("...", budget_cents=1)   # agent costs 3 cents
print(result["answered"], result["spend_cents"])
# -> 0  0
```

Run the demo:

```
cd ecosystem/kryos-embed/hosts/python
python demo_crm.py
```

### Go

```go
import "kryos-embed-go/kryosagent"

// Capability gate (returns *CapabilityViolation if over-privileged)
if err := kryosagent.ParseManifest(capsPath, []string{"ffi"}); err != nil {
    log.Fatal(err)
}

agent, err := kryosagent.NewAgent(dllPath)
// ...

resp, _ := agent.Ask("Which accounts are overdue?", 10)
fmt.Println(resp.Answered, resp.Source, resp.SpendCents)
// -> 1  mock-llm-v1  3
```

Run the demo:

```
cd ecosystem/kryos-embed/hosts/go
go run .
```

### Node (WASM sandbox)

```js
import { createAgent } from './kryos-agent.mjs';

const agent = await createAgent('./dist/kryos_embed_agent.wasm');
// prints the full import manifest (capability surface) at load

const r = agent.ask("Which accounts are overdue?", 5);
console.log(r.answered, r.source, r.spendCents);
// -> true  mock-llm-v1  3
```

Build WASM and run the demo:

```
bash ecosystem/kryos-embed/hosts/node/build.sh
node ecosystem/kryos-embed/hosts/node/demo_crm.mjs
```

### C# (recipe -- .NET SDK required)

See `hosts/csharp/README.md` for P/Invoke signatures, buffer pinning, and
`NativeLibrary.Load` usage. The governance contract is identical to Python/Go.

## Run the full integration suite

From the repo root:

```
bash ecosystem/kryos-embed/check.sh
```

This runs `build.sh` then each host's `check.sh` in sequence. Hosts whose
required runtime is absent are printed as SKIP, not FAIL.

Expected output (all present runtimes installed):

```
STAGE: build (caps-check + DLL + manifest)  -->  PASS
STAGE: python host                          -->  PASS
STAGE: go host                              -->  PASS
STAGE: node/wasm host                       -->  PASS
STAGE: csharp host                          -->  SKIP (.NET SDK not installed)

kryos-embed check.sh summary
  PASS: 4
  FAIL: 0
  SKIP: 1
RESULT: PASS
```

## Test status

| Host | Transport | Tested | Notes |
|------|-----------|--------|-------|
| Python 3 | Native DLL (ctypes) | Yes | `hosts/python/check.sh` passes |
| Go 1.25+ | Native DLL (syscall) | Yes | `hosts/go/check.sh` passes |
| Node 18+ | WASM (WebAssembly API) | Yes | `hosts/node/check.sh` passes; WASM sandbox eliminates file/TCP imports |
| C# / .NET 8+ | Native DLL (P/Invoke) | Recipe only | `hosts/csharp/` -- untested; .NET SDK not installed on this machine |

## File layout

```
ecosystem/kryos-embed/
  build.sh                  build + manifest (run first)
  check.sh                  end-to-end runner (calls build.sh + each host)
  agent/
    agent_embed.kry         Kryos agent source (DLL build)
    neg_control.kry         negative capability test (must fail E0505)
  dist/
    kryos_embed_agent.dll   compiled agent (Windows x86-64)
    agent.caps.json         authority manifest (written by build.sh)
  hosts/
    python/
      kryos_agent.py        ctypes binding + capability gate
      demo_crm.py           CRM demo
      check.sh              acceptance test
    go/
      kryosagent/agent.go   capability gate + DLL binding
      main.go               CRM demo
      check.sh              acceptance test
    node/
      kryos-agent.mjs       WASM binding + import manifest printer
      agent_wasm.kry        Kryos agent source (WASM build)
      demo_crm.mjs          CRM demo
      check.sh              acceptance test (includes WASM compile)
    csharp/
      Program.cs            P/Invoke recipe (requires .NET SDK)
      README.md             setup + ABI notes
```

## Design notes

**Why not just call an LLM API directly?**
Direct API calls send all context off-machine. This SDK runs the agent as a
compiled binary inside your process. Private data never crosses a network
boundary unless you explicitly give the agent `net:http` capability -- and that
decision is enforced by both the compiler and the host gate.

**Why Kryos for the agent?**
The Kryos compiler rejects a source file under `--strict-capabilities` if any
function uses a capability it did not declare. This means the manifest is not
self-reported -- it is verified by the compiler before the build succeeds. The
`verified: true` field in `agent.caps.json` reflects that fact.

**Mock LLM**
The current backend is a deterministic stub that returns a canned answer at a
fixed cost of 3 cents per call. Replace `mock_llm` in `agent_embed.kry` with a
real HTTP call (requires adding `net:http` to the capability declaration and
granting it in the host's allow-set).

**Provenance on every answer**
The `source` field names the backend that produced the answer. In a production
deployment this should be the model identifier from the LLM provider's response
(e.g., `"gpt-4o-2024-11-20"` or `"claude-sonnet-4-5"`), not a fixed string.
This makes audit trails verifiable without inspecting logs.
