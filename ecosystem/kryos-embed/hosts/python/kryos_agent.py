"""
kryos_agent.py -- zero-dependency ctypes binding for kryos_embed_agent.dll

Protocol (from agent.caps.json / agent_embed.kry):
  DLL exports (all i64 ABI):
    agent_call(req_ptr: i64, req_len: i64) -> i64  (returns NUL-term JSON C-ptr)
    agent_response_len() -> i64                      (byte len of last resp)

  Request JSON:  {"question": str, "budget_cents": int}
  Response JSON: {"answered": 0|1, "answer": str, "source": str,
                  "spend_cents": int, "reason": str}

Capability gate:
  Constructor reads dist/agent.caps.json BEFORE loading the DLL.
  Raises CapabilityViolation if any export demands a cap outside allowed_caps.
  The DLL is never loaded (cdll.LoadLibrary never called) when the gate fires.
"""

import ctypes
import json
import os
import pathlib
import tempfile
import shutil


class CapabilityViolation(Exception):
    """Raised when the manifest declares capabilities outside the allow-set."""


class KryosAgent:
    """
    ctypes binding to kryos_embed_agent.dll.

    Parameters
    ----------
    dll_path : str | pathlib.Path
        Absolute path to kryos_embed_agent.dll.
    caps_path : str | pathlib.Path
        Absolute path to agent.caps.json (capability manifest).
    allowed_caps : list[str]
        Capability strings that the caller consents to grant.
        Default: ['ffi'] -- minimal grant needed by this agent.

    Raises
    ------
    CapabilityViolation
        If any export in the manifest requires a capability not in allowed_caps.
        The DLL is never loaded when this fires.
    """

    def __init__(
        self,
        dll_path: str | pathlib.Path,
        caps_path: str | pathlib.Path,
        allowed_caps: list[str] | None = None,
    ):
        if allowed_caps is None:
            allowed_caps = ["ffi"]

        dll_path = pathlib.Path(dll_path).resolve()
        caps_path = pathlib.Path(caps_path).resolve()

        # --- CAPABILITY GATE (runs BEFORE any DLL load) ---
        with open(caps_path, "r", encoding="utf-8") as f:
            manifest = json.load(f)

        allowed_set = set(allowed_caps)
        violations: list[str] = []
        exports: dict = manifest.get("exports", {})
        for export_name, export_info in exports.items():
            required: list[str] = export_info.get("capabilities", [])
            for cap in required:
                if cap not in allowed_set:
                    violations.append(
                        f"export '{export_name}' requires capability '{cap}' "
                        f"which is not in allowed_caps={sorted(allowed_set)}"
                    )

        if violations:
            raise CapabilityViolation(
                "DLL NOT LOADED -- capability gate blocked:\n"
                + "\n".join(f"  - {v}" for v in violations)
            )
        # --- END CAPABILITY GATE ---

        # Load DLL only after gate passes
        self._lib = ctypes.CDLL(str(dll_path))

        # agent_call(req_ptr: i64, req_len: i64) -> i64
        self._agent_call = self._lib.agent_call
        self._agent_call.argtypes = [ctypes.c_int64, ctypes.c_int64]
        self._agent_call.restype = ctypes.c_int64

        # agent_response_len() -> i64
        self._agent_response_len = self._lib.agent_response_len
        self._agent_response_len.argtypes = []
        self._agent_response_len.restype = ctypes.c_int64

        self._manifest = manifest

    def ask(self, question: str, budget_cents: int) -> dict:
        """
        Send a question to the in-process agent.

        Parameters
        ----------
        question : str
            Natural-language question.
        budget_cents : int
            Maximum spend authorised for this call (in cents).
            The agent's budget gate fires BEFORE the mock LLM;
            a call that exceeds budget records zero spend.

        Returns
        -------
        dict with keys:
            answered   : int  (1 = answered, 0 = refused)
            answer     : str
            source     : str  (provenance tag from manifest)
            spend_cents: int
            reason     : str
        """
        # Build request JSON without Python's json.dumps to keep zero-dep feel,
        # but json.dumps is stdlib so it is fine -- "zero-dependency" means no
        # third-party packages, not no stdlib.
        req_obj = {"question": question, "budget_cents": budget_cents}
        req_str = json.dumps(req_obj)
        req_bytes = req_str.encode("utf-8")

        # Pin the request bytes in a ctypes buffer so they won't be GC'd during the call
        buf = ctypes.create_string_buffer(req_bytes, len(req_bytes))
        req_addr = ctypes.addressof(buf)

        resp_ptr_int = self._agent_call(
            ctypes.c_int64(req_addr), ctypes.c_int64(len(req_bytes))
        )
        resp_len = self._agent_response_len()

        # Read response from DLL-owned memory; resp_ptr_int is a raw i64 address
        resp_bytes = ctypes.string_at(resp_ptr_int, resp_len)
        resp_str = resp_bytes.decode("utf-8")
        return json.loads(resp_str)


# ---------------------------------------------------------------------------
# Capability-gate proof (inline unit test)
# ---------------------------------------------------------------------------

def _proof_capability_gate_blocks_on_unknown_cap(dll_path: str, caps_path: str) -> None:
    """
    Prove that a manifest with an extra capability (net:tcp) is REFUSED
    without the DLL ever loading.

    Steps:
      1. Load the real manifest.
      2. Doctor a copy: add 'net:tcp' to every export's capability list.
      3. Write the doctored copy to a temp file.
      4. Attempt to construct KryosAgent with allowed_caps=['ffi'] only.
      5. Assert CapabilityViolation is raised.
      6. Assert the DLL was NOT loaded (no ctypes handle created).
    """
    # Step 1+2: doctor the manifest
    with open(caps_path, "r", encoding="utf-8") as f:
        manifest = json.load(f)

    doctored = json.loads(json.dumps(manifest))  # deep copy
    for export_info in doctored.get("exports", {}).values():
        if "net:tcp" not in export_info.get("capabilities", []):
            export_info["capabilities"].append("net:tcp")

    # Step 3: write to temp file
    tmp_dir = tempfile.mkdtemp()
    try:
        doctored_path = os.path.join(tmp_dir, "doctored_caps.json")
        with open(doctored_path, "w", encoding="utf-8") as f:
            json.dump(doctored, f)

        # Step 4+5+6: attempt construction -- must raise CapabilityViolation
        gate_fired = False
        dll_would_load = False
        try:
            agent = KryosAgent(
                dll_path=dll_path,
                caps_path=doctored_path,
                allowed_caps=["ffi"],  # 'net:tcp' NOT in allow-set
            )
            # If we reach here the gate did NOT fire -- test failure
            dll_would_load = True
        except CapabilityViolation as exc:
            gate_fired = True
            # Verify the violation message mentions net:tcp
            assert "net:tcp" in str(exc), (
                f"CapabilityViolation raised but 'net:tcp' not in message: {exc}"
            )

        assert gate_fired, "Expected CapabilityViolation but none was raised"
        assert not dll_would_load, "DLL was loaded despite capability violation"
        print("PROOF: doctored manifest with 'net:tcp' was REFUSED before DLL load -- PASS")
    finally:
        shutil.rmtree(tmp_dir, ignore_errors=True)


if __name__ == "__main__":
    # Resolve paths relative to this file's location
    here = pathlib.Path(__file__).parent
    dist = here.parent.parent / "dist"
    dll = dist / "kryos_embed_agent.dll"
    caps = dist / "agent.caps.json"

    print("--- kryos_agent.py capability gate proof ---")
    _proof_capability_gate_blocks_on_unknown_cap(str(dll), str(caps))

    print("\n--- basic ask() smoke ---")
    agent = KryosAgent(dll_path=dll, caps_path=caps)
    result = agent.ask("What is the meaning of life?", budget_cents=5)
    print(f"answered={result['answered']} source={result['source']!r} spend={result['spend_cents']}")
    assert result["answered"] == 1, f"expected answered=1, got {result}"
    print("PASS")
