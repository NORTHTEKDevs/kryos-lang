#!/usr/bin/env python3
"""Scripted DAP client that exercises `kryos dap` end to end.

Launches `kryos.exe dap` as a subprocess, speaks Content-Length framed DAP
JSON over its stdin/stdout, and asserts the core debugger behaviour on a small
.kry program that loops a counter:

  1. initialize        -> capabilities response + `initialized` event
  2. launch            -> success
  3. setBreakpoints    -> breakpoint verified at the requested line
  4. configurationDone -> success
  5. `stopped` (breakpoint) arrives; stackTrace top frame == breakpoint line
  6. next              -> `stopped` (step) at the next line
  7. continue          -> program runs to completion (`terminated`/`exited`)

Exit code 0 = all assertions passed. No external debugger is involved; the
debuggee runs in-process via Kryos's own Cranelift JIT.
"""

import json
import os
import subprocess
import sys
import threading

HERE = os.path.dirname(os.path.abspath(__file__))


def find_exe():
    cand = os.path.join(HERE, "..", "..", "target", "release",
                        "kryos.exe" if os.name == "nt" else "kryos")
    cand = os.path.normpath(cand)
    if not os.path.exists(cand):
        print("FAIL: compiler binary not found at", cand)
        sys.exit(2)
    return cand


class DapClient:
    def __init__(self, exe, program):
        self.program = program
        self.proc = subprocess.Popen(
            [exe, "dap"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        self.seq = 0
        self.events = []          # buffered out-of-order events
        self.responses = []       # buffered out-of-order responses
        self._stderr = []
        t = threading.Thread(target=self._drain_stderr, daemon=True)
        t.start()

    def _drain_stderr(self):
        for line in self.proc.stderr:
            self._stderr.append(line.decode("utf-8", "replace"))

    def stderr_text(self):
        return "".join(self._stderr)

    def _read_message(self):
        headers = {}
        while True:
            line = self.proc.stdout.readline()
            if not line:
                return None
            line = line.decode("utf-8").rstrip("\r\n")
            if line == "":
                break
            if ":" in line:
                k, v = line.split(":", 1)
                headers[k.strip()] = v.strip()
        length = int(headers["Content-Length"])
        body = self.proc.stdout.read(length)
        return json.loads(body.decode("utf-8"))

    def send(self, command, arguments=None):
        self.seq += 1
        msg = {"seq": self.seq, "type": "request", "command": command}
        if arguments is not None:
            msg["arguments"] = arguments
        data = json.dumps(msg).encode("utf-8")
        self.proc.stdin.write(b"Content-Length: %d\r\n\r\n" % len(data))
        self.proc.stdin.write(data)
        self.proc.stdin.flush()
        return self.seq

    def response(self, command):
        for i, m in enumerate(self.responses):
            if m.get("command") == command:
                return self.responses.pop(i)
        while True:
            m = self._read_message()
            if m is None:
                raise RuntimeError(
                    "EOF awaiting response to %r\nstderr:\n%s"
                    % (command, self.stderr_text()))
            if m.get("type") == "event":
                self.events.append(m)
            elif m.get("type") == "response":
                if m.get("command") == command:
                    return m
                self.responses.append(m)

    def event(self, name):
        for i, e in enumerate(self.events):
            if e.get("event") == name:
                return self.events.pop(i)
        while True:
            m = self._read_message()
            if m is None:
                raise RuntimeError(
                    "EOF awaiting event %r\nstderr:\n%s"
                    % (name, self.stderr_text()))
            if m.get("type") == "event" and m.get("event") == name:
                return m
            if m.get("type") == "event":
                self.events.append(m)
            else:
                self.responses.append(m)

    def close(self):
        try:
            self.proc.stdin.close()
        except Exception:
            pass
        try:
            self.proc.wait(timeout=5)
        except Exception:
            self.proc.kill()


PASSED = 0


def check(cond, label, detail=""):
    global PASSED
    status = "PASS" if cond else "FAIL"
    print("  [%s] %s%s" % (status, label, ("  -- " + detail) if detail else ""))
    if not cond:
        raise AssertionError(label + " " + detail)
    PASSED += 1


def main():
    exe = find_exe()
    program = os.path.join(HERE, "counter.kry")
    bp_line = 5  # `counter = counter + i` inside the loop body

    print("kryos dap client test")
    print("  exe     :", exe)
    print("  program :", program)
    print("  bp line :", bp_line)
    print()

    c = DapClient(exe, program)
    try:
        # 1. initialize -------------------------------------------------------
        c.send("initialize", {"adapterID": "kryos", "clientID": "pytest",
                              "linesStartAt1": True, "columnsStartAt1": True})
        r = c.response("initialize")
        check(r["success"], "initialize succeeds")
        check(r["body"].get("supportsConfigurationDoneRequest") is True,
              "capabilities advertise configurationDone support")
        c.event("initialized")
        check(True, "received `initialized` event")

        # 2. launch -----------------------------------------------------------
        c.send("launch", {"program": program, "noDebug": False})
        r = c.response("launch")
        check(r["success"], "launch succeeds",
              "" if r["success"] else json.dumps(r.get("body")))

        # 3. setBreakpoints ---------------------------------------------------
        c.send("setBreakpoints", {"source": {"path": program},
                                 "breakpoints": [{"line": bp_line}]})
        r = c.response("setBreakpoints")
        bps = r["body"]["breakpoints"]
        check(len(bps) == 1 and bps[0]["verified"] is True,
              "breakpoint verified")
        check(bps[0]["line"] == bp_line,
              "breakpoint registered at line %d" % bp_line,
              "got line %s" % bps[0]["line"])

        # 4. configurationDone -> program starts ------------------------------
        c.send("configurationDone")
        check(c.response("configurationDone")["success"],
              "configurationDone succeeds")

        # 5. stopped (breakpoint) + stackTrace line ---------------------------
        ev = c.event("stopped")
        check(ev["body"]["reason"] == "breakpoint",
              "stopped event reason == breakpoint",
              "got %r" % ev["body"]["reason"])

        c.send("threads")
        thr = c.response("threads")
        check(len(thr["body"]["threads"]) >= 1, "threads returns >= 1 thread")

        c.send("stackTrace", {"threadId": 1})
        st = c.response("stackTrace")
        top = st["body"]["stackFrames"][0]
        check(top["line"] == bp_line,
              "stackTrace top frame line == breakpoint line (%d)" % bp_line,
              "got line %s (frame %r)" % (top["line"], top.get("name")))

        # 6. next -> stopped (step) at the following line ---------------------
        c.send("next", {"threadId": 1})
        c.response("next")
        ev = c.event("stopped")
        check(ev["body"]["reason"] == "step",
              "stopped event reason == step after `next`",
              "got %r" % ev["body"]["reason"])
        c.send("stackTrace", {"threadId": 1})
        st = c.response("stackTrace")
        step_line = st["body"]["stackFrames"][0]["line"]
        check(step_line == bp_line + 1,
              "single step advanced to next line (%d)" % (bp_line + 1),
              "got line %s" % step_line)

        # 7. clear breakpoints, continue to completion ------------------------
        c.send("setBreakpoints", {"source": {"path": program},
                                 "breakpoints": []})
        c.response("setBreakpoints")
        c.send("continue", {"threadId": 1})
        check(c.response("continue")["success"], "continue succeeds")

        c.event("terminated")
        check(True, "received `terminated` event (program ran to completion)")

        c.send("disconnect", {})
        c.response("disconnect")
        check(True, "disconnect acknowledged")

    finally:
        c.close()

    print()
    print("ALL %d ASSERTIONS PASSED" % PASSED)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except AssertionError as e:
        print()
        print("ASSERTION FAILED:", e)
        sys.exit(1)
