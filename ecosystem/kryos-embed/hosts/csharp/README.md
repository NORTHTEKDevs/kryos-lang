# kryos-embed: C# host recipe

**RECIPE ONLY -- requires .NET SDK (not installed on this machine; untested)**

This recipe shows how to bind `kryos_embed_agent.dll` from C# using P/Invoke.
The governance contract (capability gate, budget gate, provenance) is identical
to the tested Python and Go hosts.

## Prerequisites

- .NET SDK 8+ (`dotnet --version`)
- `kryos_embed_agent.dll` built via `bash ecosystem/kryos-embed/build.sh`
- `dist/agent.caps.json` (written by build.sh)

## Project setup

```
cd ecosystem/kryos-embed/hosts/csharp
dotnet new console --force --framework net8.0
```

Copy `Program.cs` from this directory into the project (it replaces the
generated stub).

## Run

```
dotnet run
```

Or with explicit paths:

```
dotnet run -- <absolute-path-to-dll> <absolute-path-to-caps.json>
```

## DLL placement

The P/Invoke `[DllImport("kryos_embed_agent")]` constant expects the DLL to
be resolvable at runtime. The two portable options:

**Option A** (simplest): copy the DLL next to the compiled executable.

```
cp ../../dist/kryos_embed_agent.dll bin/Debug/net8.0/
dotnet run
```

**Option B** (recommended for production): use `NativeLibrary.Load(absolutePath)`
instead of the `[DllImport]` attribute. Replace the static `KryosAgentNative`
class with a runtime-bound delegate:

```csharp
nint handle = NativeLibrary.Load(absoluteDllPath);
var agentCall = Marshal.GetDelegateForFunctionPointer<AgentCallDelegate>(
    NativeLibrary.GetExport(handle, "agent_call"));
var agentResponseLen = Marshal.GetDelegateForFunctionPointer<AgentResponseLenDelegate>(
    NativeLibrary.GetExport(handle, "agent_response_len"));
```

Where the delegate types match the proven ABI signatures:

```csharp
// All parameters and return values are int64 (long in C#).
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
delegate long AgentCallDelegate(long reqPtr, long reqLen);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
delegate long AgentResponseLenDelegate();
```

## ABI contract

Both exports use the all-`int64` C ABI verified in `dist/agent.caps.json`:

| Export | Signature |
|--------|-----------|
| `agent_call` | `(req_ptr: long, req_len: long) -> long` -- returns a NUL-terminated JSON C-string pointer (DLL-owned memory) |
| `agent_response_len` | `() -> long` -- byte length of the last response string |

Request JSON: `{"question": "...", "budget_cents": <int>}`

Response JSON:
```json
{
  "answered": 0,
  "answer": "",
  "source": "mock-llm-v1",
  "spend_cents": 0,
  "reason": "budget_cents=1 < required=3"
}
```

## Governance properties (same as Python/Go)

- **Capability gate**: `CapabilityGate.Check(capsPath, allowedCaps)` runs before
  `NativeLibrary.Load`. If the manifest declares a capability outside the
  allow-set (e.g., `net:tcp`), `CapabilityViolation` is thrown and the DLL
  is never loaded.

- **Budget gate**: pass `budget_cents` in the request JSON. The agent returns
  `answered=0` and `spend_cents=0` when the call would exceed the budget.
  No charge is recorded on refusal.

- **Provenance**: every answered response carries `"source": "mock-llm-v1"`.
  Swap for a real LLM tag when moving beyond the mock backend.

## Pinning the request buffer

The C ABI expects the request bytes to remain at a stable address for the
duration of `agent_call`. Pin them with `GCHandle`:

```csharp
GCHandle pin = GCHandle.Alloc(reqBytes, GCHandleType.Pinned);
try {
    long ptr = pin.AddrOfPinnedObject().ToInt64();
    long respPtr = agentCall(ptr, reqBytes.Length);
    // ... read response
} finally {
    pin.Free();
}
```

## Reading the response

The response pointer returned by `agent_call` points into DLL-owned memory.
Copy it out before making another call:

```csharp
long respLen = agentResponseLen();
byte[] respBytes = new byte[respLen];
Marshal.Copy(new IntPtr(respPtr), respBytes, 0, (int)respLen);
string respJson = Encoding.UTF8.GetString(respBytes);
```

## Status in the test matrix

See `ecosystem/kryos-embed/README.md` -- C# is listed as "recipe only / untested".
To promote it to "tested", install .NET SDK, run `dotnet run`, and confirm the
output matches the Python/Go demos (gate fires on doctored manifest, within-budget
call returns `answered=1 spend_cents=3`, over-budget returns `answered=0 spend_cents=0`).
