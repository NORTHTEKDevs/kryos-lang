/**
 * kryos-agent.mjs -- Node binding for the Kryos governed agent (WASM build).
 *
 * Wraps the proven host contract from tools/wasm-host/run.mjs into a
 * reusable module that exposes a single async factory + ask() method.
 *
 * WASM sandbox story
 * ------------------
 * The module has NO imports beyond the explicit host functions listed below.
 * This is verified at load time: all imports are enumerated and printed as the
 * capability manifest.  Any attempt to import a function not in this list
 * would cause WebAssembly.instantiate() to throw, blocking execution.
 *
 * Host functions provided to the sandbox (the complete import surface):
 *   env::kryos_print_i64       -- println for i64 values
 *   env::kryos_print_f64       -- println for f64 values
 *   env::kryos_print_str       -- println for str values (off, len)
 *   env::kryos_string_concat   -- str + str -> packed handle
 *   env::kryos_string_length   -- len(str)
 *   env::kryos_string_slice    -- substr(str, start, end)
 *   env::kryos_string_to_upper -- str.toUpperCase()
 *   env::kryos_string_to_lower -- str.toLowerCase()
 *   env::kryos_string_trim     -- str.trim()
 *   env::kryos_string_index_of -- str.indexOf(needle)
 *   env::kryos_string_parse_int   -- parse_int(str)
 *   env::kryos_string_parse_float -- parse_float(str)
 *   env::kryos_array_new       -- allocate array slot
 *   env::kryos_array_get       -- array element read
 *   env::kryos_array_set       -- array element write
 *   env::kryos_array_length    -- len(array)
 *   env::kryos_array_push      -- push(array, value)
 *   env::kryos_array_pop       -- pop(array)
 *   env::kryos_json_parse      -- json parse -> handle
 *   env::kryos_json_stringify  -- handle -> json str
 *   env::kryos_json_get_int    -- json int field read
 *   env::kryos_json_get_str    -- json str field read
 *   env::kryos_regex_test      -- regex match test
 *   env::kryos_regex_replace   -- regex replace
 *   env::kryos_to_string_i64   -- to_string(i64) -> packed str
 *   env::kryos_dom_set_text    -- browser no-op
 *   env::kryos_dom_get_value   -- browser no-op (returns "")
 *   env::kryos_alert           -- browser no-op
 *   env::kryos_canvas_fill_rect -- browser no-op
 *   env::kryos_canvas_clear    -- browser no-op
 *   env::kryos_fetch_text      -- browser no-op (returns "")
 *   env::kryos_http_fetch      -- browser no-op (returns "")
 *
 * NOT provided (and therefore impossible for the module to use):
 *   - File system access (no kryos_file_read / kryos_file_write)
 *   - Raw TCP/TLS sockets
 *   - Process spawn
 *   - Any WASI interface
 *
 * Usage
 * -----
 *   import { createAgent } from './kryos-agent.mjs';
 *   const agent = await createAgent('./dist/kryos_embed_agent.wasm');
 *   const result = await agent.ask('What is the meaning of life?', 5);
 *   // result: { answered: true, answer: '...', source: 'mock-llm-v1',
 *   //           spendCents: 3, reason: 'ok' }
 *
 * ask() signature
 * ---------------
 *   ask(question: string, budgetCents: number) ->
 *     { answered: boolean, answer: string, source: string,
 *       spendCents: number, reason: string }
 */

import { readFileSync } from "node:fs";

// ---------------------------------------------------------------------------
// Linear-memory helpers (identical contract to tools/wasm-host/run.mjs)
// ---------------------------------------------------------------------------

let _memory;
let _bumpPtr = 1 << 14; // start above data segment

function _bumpAlloc(bytes) {
  const pageBytes = 65536;
  while (_bumpPtr + bytes > (_memory.buffer.byteLength / pageBytes) * pageBytes) {
    _memory.grow(1);
  }
  const p = _bumpPtr;
  _bumpPtr = (_bumpPtr + bytes + 7) & ~7;
  return p;
}

const _asNum = (v) => (typeof v === "bigint" ? Number(v) : v);

function _pack(offset, len) {
  return (BigInt(len) << 32n) | BigInt(offset);
}

function _unpack(packed) {
  const p = BigInt(packed);
  return [Number(p & 0xffffffffn), Number(p >> 32n)];
}

function _readStr(off, len) {
  return new TextDecoder("utf-8").decode(
    new Uint8Array(_memory.buffer, _asNum(off), _asNum(len)),
  );
}

function _readPacked(packed) {
  const [off, len] = _unpack(packed);
  return _readStr(off, len);
}

function _writeStr(s) {
  const enc = new TextEncoder().encode(String(s));
  const off = _bumpAlloc(enc.length);
  new Uint8Array(_memory.buffer, off, enc.length).set(enc);
  return _pack(off, enc.length);
}

// ---------------------------------------------------------------------------
// JSON host table (for kryos_json_* host imports)
// ---------------------------------------------------------------------------
const _jsonTable = new Map();
let _nextJsonId = 1n;

// ---------------------------------------------------------------------------
// Host import surface (full env contract)
// ---------------------------------------------------------------------------
const _env = {
  // printing
  kryos_print_i64: (v) => process.stdout.write((typeof v === "bigint" ? v.toString() : String(v)) + "\n"),
  kryos_print_f64: (v) => process.stdout.write(String(v) + "\n"),
  kryos_print_str: (off, len) => process.stdout.write(_readStr(off, len) + "\n"),

  // strings
  kryos_string_concat: (o1, l1, o2, l2) => _writeStr(_readStr(o1, l1) + _readStr(o2, l2)),
  kryos_string_length: (off, len) => BigInt(_asNum(len)),
  kryos_string_slice: (off, len, start, end) => _writeStr(_readStr(off, len).slice(_asNum(start), _asNum(end))),
  kryos_string_to_upper: (off, len) => _writeStr(_readStr(off, len).toUpperCase()),
  kryos_string_to_lower: (off, len) => _writeStr(_readStr(off, len).toLowerCase()),
  kryos_string_trim: (off, len) => _writeStr(_readStr(off, len).trim()),
  kryos_string_index_of: (ho, hl, no, nl) => BigInt(_readStr(ho, hl).indexOf(_readStr(no, nl))),
  kryos_string_parse_int: (off, len) => { const n = parseInt(_readStr(off, len), 10); return BigInt(Number.isFinite(n) ? n : 0); },
  kryos_string_parse_float: (off, len) => { const n = parseFloat(_readStr(off, len)); return Number.isFinite(n) ? n : 0; },

  // arrays
  kryos_array_new: (count) => { const n = _asNum(count); const off = _bumpAlloc(n * 8); new Uint8Array(_memory.buffer, off, n * 8).fill(0); return _pack(off, n); },
  kryos_array_get: (packed, index) => new DataView(_memory.buffer).getBigInt64(_unpack(packed)[0] + _asNum(index) * 8, true),
  kryos_array_set: (packed, index, value) => { new DataView(_memory.buffer).setBigInt64(_unpack(packed)[0] + _asNum(index) * 8, BigInt(value), true); },
  kryos_array_length: (packed) => BigInt(_unpack(packed)[1]),
  kryos_array_push: (packed, value) => {
    const [off, len] = _unpack(packed);
    const noff = _bumpAlloc((len + 1) * 8);
    new Uint8Array(_memory.buffer, noff, len * 8).set(new Uint8Array(_memory.buffer, off, len * 8));
    new DataView(_memory.buffer).setBigInt64(noff + len * 8, BigInt(value), true);
    return _pack(noff, len + 1);
  },
  kryos_array_pop: (packed) => { const [off, len] = _unpack(packed); if (len === 0) return 0n; return new DataView(_memory.buffer).getBigInt64(off + (len - 1) * 8, true); },

  // JSON
  kryos_json_parse: (off, len) => { try { const id = _nextJsonId++; _jsonTable.set(id, JSON.parse(_readStr(off, len))); return id; } catch { return 0n; } },
  kryos_json_stringify: (handle) => { const v = _jsonTable.get(BigInt(handle)); return _writeStr(v === undefined ? "null" : JSON.stringify(v)); },
  kryos_json_get_int: (handle, ko, kl) => { const v = _jsonTable.get(BigInt(handle)); const x = v ? v[_readStr(ko, kl)] : 0; return BigInt(Number.isFinite(Number(x)) ? Math.trunc(Number(x)) : 0); },
  kryos_json_get_str: (handle, ko, kl) => { const v = _jsonTable.get(BigInt(handle)); const x = v ? v[_readStr(ko, kl)] : ""; return _writeStr(x === undefined || x === null ? "" : String(x)); },

  // regex
  kryos_regex_test: (po, pl, so, sl) => { try { return new RegExp(_readStr(po, pl)).test(_readStr(so, sl)) ? 1n : 0n; } catch { return 0n; } },
  kryos_regex_replace: (po, pl, so, sl, ro, rl) => { try { return _writeStr(_readStr(so, sl).replace(new RegExp(_readStr(po, pl), "g"), _readStr(ro, rl))); } catch { return _writeStr(_readStr(so, sl)); } },

  // to_string builtin
  kryos_to_string_i64: (v) => _writeStr(typeof v === "bigint" ? v.toString() : String(Math.trunc(Number(v)))),

  // browser-only: no-ops under Node
  kryos_dom_set_text: () => {},
  kryos_dom_get_value: () => _writeStr(""),
  kryos_alert: () => {},
  kryos_canvas_fill_rect: () => {},
  kryos_canvas_clear: () => {},
  kryos_fetch_text: () => _writeStr(""),
  kryos_http_fetch: () => _writeStr(""),
};

// ---------------------------------------------------------------------------
// Result string parser
//
// Parses the str-encoded protocol from agent_wasm.kry:
//   Within-budget: "<answer> [source=<src>,tokens=<n>,calls=<s>/<m>] SPEND:<c>"
//   Over-budget:   "REFUSED:<reason> SPEND:0"
// ---------------------------------------------------------------------------
function _parseResult(raw) {
  if (raw.startsWith("REFUSED:")) {
    // Over-budget or calls-exhausted refusal
    const reason = raw.slice("REFUSED:".length).replace(/ SPEND:0$/, "").trim();
    return { answered: false, answer: "", source: "", spendCents: 0, reason };
  }

  // Within-budget: extract source from [source=...,...]
  let source = "";
  let spendCents = 0;
  let answer = raw;

  const metaMatch = raw.match(/\[source=([^,\]]+)/);
  if (metaMatch) source = metaMatch[1];

  const spendMatch = raw.match(/ SPEND:(\d+)$/);
  if (spendMatch) spendCents = parseInt(spendMatch[1], 10);

  // Strip metadata suffix from answer
  const metaStart = raw.indexOf(" [source=");
  if (metaStart !== -1) answer = raw.slice(0, metaStart);

  return { answered: true, answer, source, spendCents, reason: "ok" };
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Load the Kryos WASM agent and return a handle with an ask() method.
 *
 * @param {string} wasmPath  - Absolute or relative path to kryos_embed_agent.wasm
 * @param {object} [options]
 * @param {boolean} [options.printImports=true]
 *   If true, print the WASM import manifest to stdout on load. This IS the
 *   capability manifest: every function the sandbox can call is listed here.
 * @returns {Promise<{ ask: function, imports: string[] }>}
 */
export async function createAgent(wasmPath, options = {}) {
  const { printImports = true } = options;

  const bytes = readFileSync(wasmPath);

  // Enumerate imports BEFORE instantiation -- this IS the capability manifest.
  const mod = await WebAssembly.compile(bytes);
  const importList = WebAssembly.Module.imports(mod).map((i) => `${i.module}::${i.name}`);

  if (printImports) {
    console.log("=== Kryos WASM agent import manifest (capability surface) ===");
    for (const imp of importList) {
      console.log("  " + imp);
    }
    console.log(`Total: ${importList.length} host imports (NO file/TCP/process access)`);
    console.log("=============================================================");
  }

  const { instance } = await WebAssembly.instantiate(bytes, { env: _env });
  _memory = instance.exports.memory;
  _bumpPtr = Math.max(_bumpPtr, 1 << 14);

  const _agentQuery = instance.exports.agent_query;
  if (typeof _agentQuery !== "function") {
    throw new Error("kryos-agent: WASM module does not export agent_query");
  }

  // Per-session call tracking
  let _callsSpent = 0;
  const MAX_CALLS_DEFAULT = 1000;

  return {
    /**
     * Ask the agent a question within a budget.
     *
     * @param {string} question     - Natural-language question
     * @param {number} budgetCents  - Max spend authorized for this call (cents)
     * @param {number} [maxCalls]   - Session call ceiling (default: 1000)
     * @returns {{ answered: boolean, answer: string, source: string,
     *             spendCents: number, reason: string }}
     */
    ask(question, budgetCents, maxCalls = MAX_CALLS_DEFAULT) {
      const qPacked = _writeStr(question);
      const resultPacked = _agentQuery(
        qPacked,
        BigInt(budgetCents),
        BigInt(_callsSpent),
        BigInt(maxCalls),
      );
      const raw = _readPacked(resultPacked);
      const result = _parseResult(raw);
      if (result.answered) {
        _callsSpent++;
      }
      return result;
    },

    /** The WASM import manifest -- every host function the sandbox can call. */
    imports: importList,
  };
}
