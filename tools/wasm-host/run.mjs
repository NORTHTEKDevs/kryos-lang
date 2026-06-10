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

const env = {
  // ---- printing ----
  kryos_print_i64: (v) => out(typeof v === "bigint" ? v.toString() : String(v)),
  kryos_print_f64: (v) => out(String(v)),
  kryos_print_str: (off, len) => out(readStr(off, len)),

  // ---- strings ----
  kryos_string_concat: (o1, l1, o2, l2) =>
    writeStr(readStr(o1, l1) + readStr(o2, l2)),
  kryos_string_length: (off, len) => BigInt(asNum(len)),
  kryos_string_slice: (off, len, start, end) => {
    const s = readStr(off, len);
    return writeStr(s.slice(asNum(start), asNum(end)));
  },
  kryos_string_to_upper: (off, len) => writeStr(readStr(off, len).toUpperCase()),
  kryos_string_to_lower: (off, len) => writeStr(readStr(off, len).toLowerCase()),
  kryos_string_trim: (off, len) => writeStr(readStr(off, len).trim()),
  kryos_string_index_of: (ho, hl, no, nl) =>
    BigInt(readStr(ho, hl).indexOf(readStr(no, nl))),
  kryos_string_parse_int: (off, len) => {
    const n = parseInt(readStr(off, len), 10);
    return BigInt(Number.isFinite(n) ? n : 0);
  },
  kryos_string_parse_float: (off, len) => {
    const n = parseFloat(readStr(off, len));
    return Number.isFinite(n) ? n : 0;
  },

  // ---- arrays (8-byte i64 slots in linear memory) ----
  kryos_array_new: (count) => {
    const n = asNum(count);
    const off = bumpAlloc(n * 8);
    new Uint8Array(memory.buffer, off, n * 8).fill(0);
    return pack(off, n);
  },
  kryos_array_get: (packed, index) => {
    const [off] = unpack(packed);
    return new DataView(memory.buffer).getBigInt64(off + asNum(index) * 8, true);
  },
  kryos_array_set: (packed, index, value) => {
    const [off] = unpack(packed);
    new DataView(memory.buffer).setBigInt64(
      off + asNum(index) * 8,
      BigInt(value),
      true,
    );
  },
  kryos_array_length: (packed) => BigInt(unpack(packed)[1]),
  kryos_array_push: (packed, value) => {
    // Reallocate len+1 slots, copy, append. Handles are immutable packs.
    const [off, len] = unpack(packed);
    const noff = bumpAlloc((len + 1) * 8);
    new Uint8Array(memory.buffer, noff, len * 8).set(
      new Uint8Array(memory.buffer, off, len * 8),
    );
    new DataView(memory.buffer).setBigInt64(noff + len * 8, BigInt(value), true);
    return pack(noff, len + 1);
  },
  kryos_array_pop: (packed) => {
    const [off, len] = unpack(packed);
    if (len === 0) return 0n;
    return new DataView(memory.buffer).getBigInt64(off + (len - 1) * 8, true);
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
