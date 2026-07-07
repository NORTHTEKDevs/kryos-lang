#!/usr/bin/env node
// Kryos WASM host harness for Node (CI smoke + local runs).
//
// The Kryos `--backend wasm` modules import their runtime from the `env`
// host module (see examples/wasm_web_runner.html for the browser version
// of this contract). They are NOT WASI modules — `options.wasi` in
// kryos-codegen-wasm is unimplemented scaffolding — so wasmtime cannot run
// them ("unknown import: env::kryos_print_i64"). This harness provides the
// full env import surface in Node: prints go to stdout, browser-only
// functions (DOM / canvas / alert) are no-ops, and http/fetch return "".
//
// Usage: node tools/wasm-host/run.mjs <module.wasm>
//
// Conventions (mirroring the web runner):
//   - strings are (offset, len) pairs in linear memory
//   - a packed string/array handle is (BigInt(len) << 32n) | BigInt(offset)
//   - data segment at 0..N holds string literals; bump allocator above 16KB

import { readFileSync } from "node:fs";

const file = process.argv[2];
if (!file) {
  console.error("usage: node tools/wasm-host/run.mjs <module.wasm>");
  process.exit(2);
}

let memory;
let bumpPtr = 1 << 14;
// Host-side map storage: handle N -> maps[N-1] (a JS Map<string, bigint>).
const maps = [];
// Host-side array storage: handle N -> arrays[N-1] (a JS array of BigInt).
// Host-backed so push/pop mutate in place, matching the native backends.
const arrays = [];

function bumpAlloc(bytes) {
  const pageBytes = 65536;
  while (bumpPtr + bytes > (memory.buffer.byteLength / pageBytes) * pageBytes) {
    memory.grow(1);
  }
  const p = bumpPtr;
  bumpPtr = (bumpPtr + bytes + 7) & ~7;
  return p;
}

const asNum = (v) => (typeof v === "bigint" ? Number(v) : v);

function pack(offset, len) {
  return (BigInt(len) << 32n) | BigInt(offset);
}

function unpack(packed) {
  const p = BigInt(packed);
  return [Number(p & 0xffffffffn), Number(p >> 32n)];
}

function readStr(off, len) {
  return new TextDecoder("utf-8").decode(
    new Uint8Array(memory.buffer, asNum(off), asNum(len)),
  );
}

function readPacked(packed) {
  const [off, len] = unpack(packed);
  return readStr(off, len);
}

function writeStr(s) {
  const enc = new TextEncoder().encode(String(s));
  const off = bumpAlloc(enc.length);
  new Uint8Array(memory.buffer, off, enc.length).set(enc);
  return pack(off, enc.length);
}

const out = (line) => process.stdout.write(line + "\n");

// JSON handles: parsed values live host-side in a table keyed by id.
const jsonTable = new Map();
let nextJsonId = 1n;

// Character interning: ensures kryos_string_char_at returns the same packed
// handle for the same byte value, so packed-i64 equality (used for string
// char comparison in split/contains) works correctly.
const charHandles = new Map();

const env = {
  // ---- printing ----
  kryos_print_i64: (v) => out(typeof v === "bigint" ? v.toString() : String(v)),
  kryos_print_f64: (v) => out(String(v)),
  kryos_print_str: (off, len) => out(readStr(off, len)),

  // ---- strings ----
  kryos_string_concat: (o1, l1, o2, l2) =>
    writeStr(readStr(o1, l1) + readStr(o2, l2)),
  kryos_string_length: (packed) => unpack(packed)[1],
  kryos_string_slice: (packed, start, end) => {
    const [off, len] = unpack(packed);
    const s = readStr(off, len);
    return writeStr(s.slice(asNum(start), asNum(end)));
  },
  kryos_string_char_at: (packed, idx) => {
    const [off, len] = unpack(packed);
    const i = asNum(idx);
    if (i < 0 || i >= len) return writeStr("");
    const byte = new Uint8Array(memory.buffer)[off + i];
    if (!charHandles.has(byte)) {
      charHandles.set(byte, writeStr(String.fromCharCode(byte)));
    }
    return charHandles.get(byte);
  },
  kryos_string_retain: (packed) => packed,
  kryos_string_char_code: (packed) => {
    const [off, len] = unpack(packed);
    if (len === 0) return 0n;
    return BigInt(new Uint8Array(memory.buffer)[off]);
  },
  kryos_string_contains: (hay, needle) => {
    return readPacked(hay).includes(readPacked(needle)) ? 1n : 0n;
  },
  kryos_string_to_upper: (off, len) => writeStr(readStr(off, len).toUpperCase()),
  kryos_string_to_lower: (off, len) => writeStr(readStr(off, len).toLowerCase()),
  kryos_string_trim: (off, len) => writeStr(readStr(off, len).trim()),
  kryos_string_index_of: (ho, hl, no, nl) =>
    BigInt(readStr(ho, hl).indexOf(readStr(no, nl))),
  kryos_string_parse_int: (packed) => {
    const [off, len] = unpack(packed);
    const n = parseInt(readStr(off, len), 10);
    return BigInt(Number.isFinite(n) ? n : 0);
  },
  kryos_string_parse_float: (packed) => {
    const [off, len] = unpack(packed);
    const n = parseFloat(readStr(off, len));
    return Number.isFinite(n) ? n : 0;
  },

  // ---- arrays (8-byte i64 slots in linear memory) ----
  // ---- Arrays: host-backed (a JS array of BigInt per handle), so push/pop
  // MUTATE IN PLACE -- matching the native backends, where `push(arr, x)` with
  // the result discarded grows arr. Handle = 1-based index into `arrays`. ----
  kryos_array_new: (count) => {
    arrays.push(new Array(asNum(count)).fill(0n));
    return BigInt(arrays.length); // 1-based, nonzero
  },
  kryos_array_get: (handle, index) => arrays[asNum(handle) - 1][asNum(index)],
  kryos_array_set: (handle, index, value) => {
    arrays[asNum(handle) - 1][asNum(index)] = BigInt(value);
  },
  kryos_array_length: (handle) => arrays[asNum(handle) - 1].length,
  // ---- Maps (str-keyed, i64-valued). Handle = 1-based index into `maps`. ----
  kryos_map_new: () => {
    maps.push(new Map());
    return BigInt(maps.length); // 1-based, nonzero
  },
  kryos_map_insert_str: (handle, keyPacked, value) => {
    const m = maps[asNum(handle) - 1];
    const [off, len] = unpack(keyPacked);
    m.set(readStr(off, len), BigInt(value));
    return handle; // passthrough
  },
  kryos_map_get_str: (handle, keyPacked) => {
    const m = maps[asNum(handle) - 1];
    const [off, len] = unpack(keyPacked);
    const v = m.get(readStr(off, len));
    return v === undefined ? 0n : v;
  },
  kryos_map_has_str: (handle, keyPacked) => {
    const m = maps[asNum(handle) - 1];
    const [off, len] = unpack(keyPacked);
    return m.has(readStr(off, len)) ? 1n : 0n;
  },
  kryos_map_len: (handle) => maps[asNum(handle) - 1].size,
  kryos_array_push: (handle, value) => {
    // In-place append (mutates the existing array), returns the SAME handle so
    // both `push(a, x)` (discarded) and `a = push(a, x)` (reassigned) work.
    arrays[asNum(handle) - 1].push(BigInt(value));
    return handle;
  },
  kryos_array_pop: (handle) => {
    const a = arrays[asNum(handle) - 1];
    return a.length === 0 ? 0n : a.pop();
  },

  // ---- JSON ----
  kryos_json_parse: (off, len) => {
    try {
      const id = nextJsonId++;
      jsonTable.set(id, JSON.parse(readStr(off, len)));
      return id;
    } catch {
      return 0n;
    }
  },
  kryos_json_stringify: (handle) => {
    const v = jsonTable.get(BigInt(handle));
    return writeStr(v === undefined ? "null" : JSON.stringify(v));
  },
  kryos_json_get_int: (handle, ko, kl) => {
    const v = jsonTable.get(BigInt(handle));
    const x = v ? v[readStr(ko, kl)] : 0;
    return BigInt(Number.isFinite(Number(x)) ? Math.trunc(Number(x)) : 0);
  },
  kryos_json_get_str: (handle, ko, kl) => {
    const v = jsonTable.get(BigInt(handle));
    const x = v ? v[readStr(ko, kl)] : "";
    return writeStr(x === undefined || x === null ? "" : String(x));
  },

  // ---- regex ----
  kryos_regex_test: (po, pl, so, sl) => {
    try {
      return new RegExp(readStr(po, pl)).test(readStr(so, sl)) ? 1n : 0n;
    } catch {
      return 0n;
    }
  },
  kryos_regex_replace: (po, pl, so, sl, ro, rl) => {
    try {
      return writeStr(
        readStr(so, sl).replace(new RegExp(readStr(po, pl), "g"), readStr(ro, rl)),
      );
    } catch {
      return writeStr(readStr(so, sl));
    }
  },

  // ---- to_string: convert i64/f64 to their decimal string representations ----
  kryos_to_string_i64: (v) => writeStr(typeof v === "bigint" ? v.toString() : String(Math.trunc(Number(v)))),
  kryos_to_string_f64: (v) => writeStr(String(v)),

  // ---- browser-only surface: no-ops / empty results under Node ----
  kryos_dom_set_text: () => {},
  kryos_dom_get_value: () => writeStr(""),
  kryos_alert: () => {},
  kryos_canvas_fill_rect: () => {},
  kryos_canvas_clear: () => {},
  kryos_fetch_text: () => writeStr(""),
  kryos_http_fetch: () => writeStr(""),
};

const bytes = readFileSync(file);
const { instance } = await WebAssembly.instantiate(bytes, { env });
memory = instance.exports.memory;
bumpPtr = Math.max(bumpPtr, 1 << 14);

if (typeof instance.exports.main !== "function") {
  console.error("wasm-host: module exports no main()");
  process.exit(1);
}
instance.exports.main();
