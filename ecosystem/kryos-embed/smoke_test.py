"""
ecosystem/kryos-embed/smoke_test.py

End-to-end smoke test for the kryos_embed_agent DLL.
Run from the repo root:
    python3 ecosystem/kryos-embed/smoke_test.py

Tests:
  (a) Within-budget call (budget_cents=10 >= cost=3): answered=1, source present
  (b) Over-budget call  (budget_cents=1  <  cost=3): answered=0, spend_cents=0
"""

import ctypes
import json
import os
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DLL_PATH = os.path.join(REPO_ROOT, "ecosystem", "kryos-embed", "dist", "kryos_embed_agent.dll")

print(f"Loading DLL: {DLL_PATH}")
dll = ctypes.CDLL(DLL_PATH)

# Configure function signatures (all i64 / c_int64)
dll.agent_call.restype = ctypes.c_int64
dll.agent_call.argtypes = [ctypes.c_int64, ctypes.c_int64]
dll.agent_response_len.restype = ctypes.c_int64
dll.agent_response_len.argtypes = []

def call_agent(request_dict):
    """Call agent_call with a JSON request dict; return parsed response dict."""
    req_bytes = json.dumps(request_dict).encode("utf-8")
    buf = ctypes.create_string_buffer(req_bytes)
    req_ptr = ctypes.cast(buf, ctypes.c_char_p)
    req_ptr_int = ctypes.cast(req_ptr, ctypes.c_void_p).value

    resp_ptr = dll.agent_call(req_ptr_int, len(req_bytes))
    resp_len = dll.agent_response_len()

    # Read resp_len bytes from the returned C-string pointer
    raw = ctypes.string_at(resp_ptr, resp_len)
    return json.loads(raw.decode("utf-8"))

fail = False

# (a) Within-budget call: budget_cents=10 >= MOCK_COST_CENTS=3
print("\n[a] Within-budget call (budget_cents=10)")
resp_a = call_agent({"question": "what is the meaning of life?", "budget_cents": 10})
print(f"    response: {resp_a}")

assert resp_a["answered"] == 1, f"Expected answered=1, got {resp_a['answered']}"
assert resp_a["source"] != "", f"Expected non-empty source, got empty"
assert resp_a["spend_cents"] == 3, f"Expected spend_cents=3, got {resp_a['spend_cents']}"
assert resp_a["answer"] != "", f"Expected non-empty answer"
print("    PASS: answered=1, source present, spend_cents=3")

# (b) Over-budget call: budget_cents=1 < MOCK_COST_CENTS=3
print("\n[b] Over-budget call (budget_cents=1)")
resp_b = call_agent({"question": "what is the meaning of life?", "budget_cents": 1})
print(f"    response: {resp_b}")

assert resp_b["answered"] == 0, f"Expected answered=0, got {resp_b['answered']}"
assert resp_b["spend_cents"] == 0, f"Expected spend_cents=0, got {resp_b['spend_cents']}"
assert resp_b["reason"] != "", f"Expected non-empty reason"
print("    PASS: answered=0, spend_cents=0, reason present")

print("\nsmoke_test: ALL ASSERTIONS PASSED")
