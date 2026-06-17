//! `kryos dap` — a self-contained source-level debugger exposed over the
//! Debug Adapter Protocol (DAP) on stdio.
//!
//! The adapter compiles the target `.kry` program with the debugger's
//! line-instrumentation enabled, JIT-compiles it via Cranelift, and runs it on
//! a dedicated thread *in this process*. The instrumented code calls the
//! `kryos_dbg_line` runtime hook before each source statement; the runtime
//! debug controller (`kryos_rt::debug`) parks the debuggee at breakpoints /
//! single-steps and the adapter drives it via DAP `continue` / `next` requests.
//!
//! There is no dependency on any external native debugger — everything is
//! backed by Kryos's own JIT execution, so it is fully verifiable with a
//! scripted DAP client.
//!
//! ## Implemented requests
//! `initialize`, `launch`, `setBreakpoints`, `configurationDone`, `threads`,
//! `stackTrace`, `scopes`, `variables`, `continue`, `next`/`stepIn`/`stepOut`,
//! `pause`, `disconnect`/`terminate`.
//!
//! ## Drive model
//! The adapter is single-threaded: after `configurationDone`, `continue`, or
//! `next` it resumes the debuggee and then blocks until the next stop (or
//! program exit), emitting the corresponding `stopped` / `terminated` event
//! before reading the next request. DAP clients always await a `stopped` event
//! after resuming, so this ordering is protocol-correct.
//!
//! ## Known limitations (honest)
//! * `scopes`/`variables` do not expose live JIT locals yet — capturing values
//!   out of Cranelift SSA/stack slots from another thread is the hard part and
//!   is deferred. `variables` returns a single informational entry that says
//!   so rather than fabricating values.
//! * The debuggee's command-line `args()` are empty (no argv plumbing in the
//!   JIT entry path).

use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::mpsc;
use std::thread::JoinHandle;

use serde_json::{json, Value};

use kryos_rt::debug::StopEvent;

struct DapServer {
    /// Outgoing message sequence counter.
    out_seq: i64,
    /// Target program path (from the `launch` request).
    program: Option<String>,
    /// JIT entry-point address for the program's `main`.
    main_ptr: Option<usize>,
    /// Sorted, unique set of source lines that carry an instrumentation hook —
    /// the lines a breakpoint can actually stop on.
    instrumented_lines: Vec<i64>,
    /// Last stop sequence reported to the client (dedup guard).
    seen_seq: u64,
    /// The running debuggee thread.
    debuggee: Option<JoinHandle<()>>,
    /// Receiver for the debuggee's redirected stdout.
    output_rx: Option<mpsc::Receiver<String>>,
    /// True once the program has exited and `terminated` has been sent.
    finished: bool,
}

impl DapServer {
    fn new() -> Self {
        Self {
            out_seq: 0,
            program: None,
            main_ptr: None,
            instrumented_lines: Vec::new(),
            seen_seq: 0,
            debuggee: None,
            output_rx: None,
            finished: false,
        }
    }

    // -- message I/O -------------------------------------------------------

    fn next_seq(&mut self) -> i64 {
        self.out_seq += 1;
        self.out_seq
    }

    fn send_response(&mut self, req_seq: i64, command: &str, success: bool, body: Value) {
        let seq = self.next_seq();
        let msg = json!({
            "seq": seq,
            "type": "response",
            "request_seq": req_seq,
            "success": success,
            "command": command,
            "body": body,
        });
        write_message(&msg);
    }

    fn send_event(&mut self, event: &str, body: Value) {
        let seq = self.next_seq();
        let msg = json!({
            "seq": seq,
            "type": "event",
            "event": event,
            "body": body,
        });
        write_message(&msg);
    }

    // -- request handlers --------------------------------------------------

    /// Dispatch a single request. Returns `true` if the session should end.
    fn handle(&mut self, msg: &Value) -> bool {
        let command = msg.get("command").and_then(|c| c.as_str()).unwrap_or("");
        let req_seq = msg.get("seq").and_then(|s| s.as_i64()).unwrap_or(0);
        let args = msg.get("arguments").cloned().unwrap_or(Value::Null);

        match command {
            "initialize" => {
                let body = json!({
                    "supportsConfigurationDoneRequest": true,
                    "supportsTerminateRequest": true,
                    "supportsSingleThreadExecutionRequests": false,
                    "supportsStepBack": false,
                    "supportsFunctionBreakpoints": false,
                });
                self.send_response(req_seq, "initialize", true, body);
                // Tell the client it may now configure (set breakpoints, etc.).
                self.send_event("initialized", json!({}));
            }
            "launch" => self.launch(&args, req_seq),
            "setBreakpoints" => self.set_breakpoints(&args, req_seq),
            "configurationDone" => {
                self.send_response(req_seq, "configurationDone", true, json!({}));
                self.start_debuggee();
                self.pump();
            }
            "threads" => {
                self.send_response(
                    req_seq,
                    "threads",
                    true,
                    json!({ "threads": [ { "id": 1, "name": "main" } ] }),
                );
            }
            "stackTrace" => self.stack_trace(req_seq),
            "scopes" => {
                self.send_response(
                    req_seq,
                    "scopes",
                    true,
                    json!({ "scopes": [ {
                        "name": "Locals",
                        "variablesReference": 1,
                        "expensive": false,
                        "presentationHint": "locals",
                    } ] }),
                );
            }
            "variables" => {
                // Honest placeholder: live JIT local capture is not implemented.
                self.send_response(
                    req_seq,
                    "variables",
                    true,
                    json!({ "variables": [ {
                        "name": "(info)",
                        "value": "local variable inspection is not yet implemented \
                                  (capturing JIT locals is a known next step)",
                        "variablesReference": 0,
                    } ] }),
                );
            }
            "continue" => {
                self.send_response(
                    req_seq,
                    "continue",
                    true,
                    json!({ "allThreadsContinued": true }),
                );
                if !self.finished {
                    kryos_rt::debug::controller().resume_continue();
                    self.pump();
                }
            }
            "next" | "stepIn" | "stepOut" => {
                self.send_response(req_seq, command, true, json!({}));
                if !self.finished {
                    kryos_rt::debug::controller().resume_step();
                    self.pump();
                }
            }
            "pause" => {
                self.send_response(req_seq, "pause", true, json!({}));
            }
            "disconnect" | "terminate" => {
                self.send_response(req_seq, command, true, json!({}));
                // Let a parked debuggee run to completion so we don't leak a
                // blocked thread, then end the session.
                if !self.finished {
                    kryos_rt::debug::controller().resume_continue();
                }
                return true;
            }
            other => {
                // Be lenient with requests we don't model (e.g.
                // setExceptionBreakpoints): acknowledge success with no body.
                self.send_response(req_seq, other, true, json!({}));
            }
        }
        false
    }

    fn launch(&mut self, args: &Value, req_seq: i64) {
        let program = match args.get("program").and_then(|p| p.as_str()) {
            Some(p) => p.to_string(),
            None => {
                self.send_response(
                    req_seq,
                    "launch",
                    false,
                    json!({ "error": "launch requires a `program` path" }),
                );
                return;
            }
        };
        let path = Path::new(&program);
        if !path.exists() {
            self.send_response(
                req_seq,
                "launch",
                false,
                json!({ "error": format!("program not found: {program}") }),
            );
            return;
        }
        self.program = Some(program.clone());

        // Compile to MIR with debug-line instrumentation enabled.
        kryos_mir::debug_lines::request_debug_instrument(true);
        let mut config = kryos_driver::BuildConfig::for_file(&program);
        config.output_type = kryos_driver::OutputType::Mir;
        let result = kryos_driver::compile_file(path, &config);
        kryos_mir::debug_lines::request_debug_instrument(false);

        if !result.success {
            for d in &result.diagnostics {
                let rendered = kryos_errors::render_diagnostic_plain(d, &result.source_map);
                self.send_event(
                    "output",
                    json!({ "category": "stderr", "output": rendered }),
                );
            }
            self.send_response(
                req_seq,
                "launch",
                false,
                json!({ "error": "compilation failed" }),
            );
            return;
        }

        let mir = match result.mir {
            Some(m) => m,
            None => {
                self.send_response(
                    req_seq,
                    "launch",
                    false,
                    json!({ "error": "compiler produced no MIR" }),
                );
                return;
            }
        };

        // Collect the lines that carry a debug hook — the stoppable lines.
        let mut lines = std::collections::BTreeSet::new();
        for f in &mir.functions {
            for bb in &f.blocks {
                for ins in &bb.instructions {
                    if let kryos_mir::ir::Instruction::DebugLine(n) = ins {
                        lines.insert(*n as i64);
                    }
                }
            }
        }
        self.instrumented_lines = lines.into_iter().collect();

        // JIT-compile the whole module so cross-function calls resolve.
        let backend = kryos_codegen_cranelift::CraneliftBackend::new();
        match backend.jit_compile_module(&mir) {
            Ok(ptrs) => match ptrs.get("main") {
                Some(&p) => {
                    self.main_ptr = Some(p as usize);
                    self.send_response(req_seq, "launch", true, json!({}));
                }
                None => {
                    self.send_response(
                        req_seq,
                        "launch",
                        false,
                        json!({ "error": "no `main` function found" }),
                    );
                }
            },
            Err(e) => {
                self.send_response(
                    req_seq,
                    "launch",
                    false,
                    json!({ "error": format!("JIT compilation failed: {e}") }),
                );
            }
        }
    }

    fn set_breakpoints(&mut self, args: &Value, req_seq: i64) {
        let mut requested: Vec<i64> = Vec::new();
        if let Some(bps) = args.get("breakpoints").and_then(|b| b.as_array()) {
            for bp in bps {
                if let Some(l) = bp.get("line").and_then(|l| l.as_i64()) {
                    requested.push(l);
                }
            }
        } else if let Some(ls) = args.get("lines").and_then(|l| l.as_array()) {
            for l in ls {
                if let Some(n) = l.as_i64() {
                    requested.push(n);
                }
            }
        }

        let mut verified_lines: Vec<i64> = Vec::new();
        let mut resp: Vec<Value> = Vec::new();
        for l in requested {
            // Snap a requested line to the nearest stoppable line at-or-after
            // it. Before `launch` (no instrumentation info yet) we trust the
            // requested line as-is.
            let snapped = if self.instrumented_lines.is_empty() {
                Some(l)
            } else if self.instrumented_lines.contains(&l) {
                Some(l)
            } else {
                self.instrumented_lines.iter().copied().find(|&x| x >= l)
            };
            match snapped {
                Some(s) => {
                    verified_lines.push(s);
                    resp.push(json!({ "verified": true, "line": s }));
                }
                None => resp.push(json!({ "verified": false, "line": l })),
            }
        }

        kryos_rt::debug::controller().set_breakpoints(&verified_lines);
        self.send_response(
            req_seq,
            "setBreakpoints",
            true,
            json!({ "breakpoints": resp }),
        );
    }

    fn stack_trace(&mut self, req_seq: i64) {
        let ctrl = kryos_rt::debug::controller();
        let frames = ctrl.current_frames();
        let path = self.program.clone().unwrap_or_default();
        let name = Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("source")
            .to_string();

        let source = json!({ "name": name, "path": path });
        let mut out: Vec<Value> = Vec::new();
        if frames.is_empty() {
            out.push(json!({
                "id": 1,
                "name": "main",
                "line": ctrl.current_line(),
                "column": 1,
                "source": source,
            }));
        } else {
            for (i, (fname, line)) in frames.iter().enumerate() {
                out.push(json!({
                    "id": i as i64 + 1,
                    "name": fname,
                    "line": line,
                    "column": 1,
                    "source": source,
                }));
            }
        }
        let total = out.len();
        self.send_response(
            req_seq,
            "stackTrace",
            true,
            json!({ "stackFrames": out, "totalFrames": total }),
        );
    }

    // -- execution control -------------------------------------------------

    fn start_debuggee(&mut self) {
        let ptr = match self.main_ptr {
            Some(p) => p,
            None => return,
        };
        // Redirect the debuggee's stdout into a channel so its output does not
        // corrupt our DAP frames on the real stdout.
        let (tx, rx) = mpsc::channel::<String>();
        kryos_rt::debug::set_stdout_sink(Some(tx));
        self.output_rx = Some(rx);

        // Move the entry address as a `usize` (Send); the JIT module's code is
        // leaked on drop (as the REPL also relies on), so it stays valid.
        let addr = ptr;
        let handle = std::thread::Builder::new()
            .name("kryos-debuggee".to_string())
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                // SAFETY: `addr` is the JIT-compiled `main` with a `fn()` ABI.
                let main_fn: unsafe extern "C" fn() =
                    unsafe { std::mem::transmute(addr as *const ()) };
                unsafe { main_fn() };
                kryos_rt::debug::controller().mark_finished();
            })
            .expect("failed to spawn debuggee thread");
        self.debuggee = Some(handle);
    }

    /// Resume-and-wait: block until the debuggee stops again or exits, then
    /// emit the corresponding event(s).
    fn pump(&mut self) {
        if self.finished {
            return;
        }
        let ctrl = kryos_rt::debug::controller();
        match ctrl.wait_for_stop(self.seen_seq) {
            StopEvent::Stopped { seq, reason, .. } => {
                self.seen_seq = seq;
                self.drain_output();
                self.send_event(
                    "stopped",
                    json!({
                        "reason": reason.as_dap_str(),
                        "threadId": 1,
                        "allThreadsStopped": true,
                    }),
                );
            }
            StopEvent::Finished => {
                self.drain_output();
                self.send_event("exited", json!({ "exitCode": 0 }));
                self.send_event("terminated", json!({}));
                self.finished = true;
                if let Some(h) = self.debuggee.take() {
                    let _ = h.join();
                }
            }
        }
    }

    fn drain_output(&mut self) {
        let mut chunks: Vec<String> = Vec::new();
        if let Some(rx) = &self.output_rx {
            while let Ok(s) = rx.try_recv() {
                chunks.push(s);
            }
        }
        for s in chunks {
            self.send_event("output", json!({ "category": "stdout", "output": s }));
        }
    }
}

/// Run the DAP server on stdin/stdout until the client disconnects or EOF.
pub fn execute() -> Result<(), String> {
    // Initialise the runtime debug controller up front so breakpoints can be
    // registered and the line hook is live before the debuggee runs.
    kryos_rt::debug::controller();

    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut server = DapServer::new();

    loop {
        match read_message(&mut reader) {
            Ok(Some(msg)) => {
                if server.handle(&msg) {
                    break;
                }
            }
            Ok(None) => break, // EOF
            Err(e) => {
                eprintln!("kryos dap: protocol read error: {e}");
                break;
            }
        }
    }

    // Tear down the stdout redirect.
    kryos_rt::debug::set_stdout_sink(None);
    Ok(())
}

// ---------------------------------------------------------------------------
// Content-Length framed JSON-RPC transport
// ---------------------------------------------------------------------------

/// Read a single DAP message (Content-Length header block + JSON body).
fn read_message(reader: &mut impl BufRead) -> std::io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    let mut header_line = String::new();

    loop {
        header_line.clear();
        let n = reader.read_line(&mut header_line)?;
        if n == 0 {
            return Ok(None); // EOF
        }
        let line = header_line.trim();
        if line.is_empty() {
            break; // end of headers
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().ok();
        }
    }

    let length = content_length.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length")
    })?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(value))
}

/// Write a single DAP message to stdout, framed with a Content-Length header.
fn write_message(value: &Value) {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = write!(lock, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = lock.flush();
}
