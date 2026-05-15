// Minimal WASM host runner for Kryos-emitted modules.
//
// Usage: node wasm_runner.js <path-to.wasm>
//
// Provides the host imports the WASM backend expects (v0.2):
//   env.kryos_print_i64(i64)
//   env.kryos_print_f64(f64)
//   env.kryos_print_str(offset, len)
//   env.kryos_string_concat(off1, len1, off2, len2) -> i64  (packed)
//   env.kryos_array_new(count) -> i64                       (packed)
//   env.kryos_array_get(packed, index) -> i64
//   env.kryos_array_set(packed, index, value)
//
// Reads/writes strings and arrays in the module's exported `memory`.

const fs = require('fs');

// Pack a 32-bit offset and a 32-bit length into one BigInt for the i64 return.
function pack(offset, len) {
  return (BigInt(len) << 32n) | (BigInt(offset) & 0xffffffffn);
}

// Bump-allocator state for host-allocated objects (concat results, arrays).
// We start above the static-data area; Kryos's WASM data segment never
// exceeds a few KB for typical programs, so we begin at offset 32 KB and
// grow upward through linear memory. Each module gets a fresh runner so
// state is per-process.
let heapPtr = 32 * 1024;

function bumpAlloc(memory, nBytes) {
  // Make sure linear memory has enough pages. Page = 64 KB.
  const needed = heapPtr + nBytes;
  const currentBytes = memory.buffer.byteLength;
  if (needed > currentBytes) {
    const extraPages = Math.ceil((needed - currentBytes) / 65536);
    memory.grow(extraPages);
  }
  const out = heapPtr;
  heapPtr += nBytes;
  // 8-byte align to be safe for i64 arrays.
  heapPtr = (heapPtr + 7) & ~7;
  return out;
}

async function main() {
  const wasmPath = process.argv[2];
  if (!wasmPath) {
    console.error('usage: node wasm_runner.js <file.wasm>');
    process.exit(1);
  }

  const bytes = fs.readFileSync(wasmPath);

  let memory = null;

  const imports = {
    env: {
      kryos_print_i64(v) {
        console.log(v.toString());
      },
      kryos_print_f64(v) {
        console.log(v);
      },
      kryos_print_str(offset, len) {
        if (!memory) {
          console.error('kryos_print_str called before memory available');
          return;
        }
        const view = new Uint8Array(memory.buffer, offset, len);
        const text = new TextDecoder('utf-8').decode(view);
        console.log(text);
      },

      // v0.2: concatenate two strings stored in linear memory.
      // Returns a packed (offset|len<<32) i64.
      kryos_string_concat(off1, len1, off2, len2) {
        const totalLen = len1 + len2;
        const newOffset = bumpAlloc(memory, totalLen);
        const dst = new Uint8Array(memory.buffer);
        const src1 = new Uint8Array(memory.buffer, off1, len1);
        const src2 = new Uint8Array(memory.buffer, off2, len2);
        dst.set(src1, newOffset);
        dst.set(src2, newOffset + len1);
        return pack(newOffset, totalLen);
      },

      // v0.2: allocate a fresh i64 array of `count` elements, zero-initialized.
      // Returns a packed (offset|count<<32) i64. Each element is 8 bytes.
      kryos_array_new(count) {
        const bytes = count * 8;
        const offset = bumpAlloc(memory, bytes);
        // bumpAlloc grows memory if needed; zero-fill the region.
        const view = new Uint8Array(memory.buffer, offset, bytes);
        view.fill(0);
        return pack(offset, count);
      },

      // v0.2: read i64 element at `index` from packed array `packed`.
      kryos_array_get(packed, index) {
        // packed is a BigInt (i64 import). Unpack offset = low 32 bits.
        const offset = Number(packed & 0xffffffffn);
        const dv = new DataView(memory.buffer);
        return dv.getBigInt64(offset + index * 8, true);
      },

      // v0.2: write `value` (i64) into packed array at `index`.
      kryos_array_set(packed, index, value) {
        const offset = Number(packed & 0xffffffffn);
        const dv = new DataView(memory.buffer);
        dv.setBigInt64(offset + index * 8, value, true);
      },

      // ---- WASM v0.4: web host stubs (node fallback) ----
      kryos_dom_set_text(idOff, idLen, txtOff, txtLen) {
        const id = readStr(memory, idOff, idLen);
        const txt = readStr(memory, txtOff, txtLen);
        console.error(`[dom_set_text] #${id} <- ${txt}`);
      },
      kryos_dom_get_value(idOff, idLen) {
        const id = readStr(memory, idOff, idLen);
        console.error(`[dom_get_value] #${id} -> (node stub: empty)`);
        return writeStrToMem(memory, '');
      },
      kryos_alert(off, len) {
        console.error(`[alert] ${readStr(memory, off, len)}`);
      },
      kryos_canvas_fill_rect(idOff, idLen, x, y, w, h, cOff, cLen) {
        const id = readStr(memory, idOff, idLen);
        const c = readStr(memory, cOff, cLen);
        console.error(`[canvas_fill_rect] #${id} (${x},${y}) ${w}x${h} ${c}`);
      },
      kryos_canvas_clear(idOff, idLen) {
        const id = readStr(memory, idOff, idLen);
        console.error(`[canvas_clear] #${id}`);
      },
      kryos_fetch_text(urlOff, urlLen) {
        const url = readStr(memory, urlOff, urlLen);
        console.error(`[fetch_text] ${url} -> (node stub: empty)`);
        return writeStrToMem(memory, '');
      },
    },
  };

  function readStr(mem, off, len) {
    const o = typeof off === 'bigint' ? Number(off) : off;
    const l = typeof len === 'bigint' ? Number(len) : len;
    return new TextDecoder('utf-8').decode(new Uint8Array(mem.buffer, o, l));
  }
  function writeStrToMem(mem, s) {
    const enc = new TextEncoder().encode(s);
    const off = bumpAlloc(mem, enc.length);
    new Uint8Array(mem.buffer, off, enc.length).set(enc);
    return pack(off, enc.length);
  }

  const { instance } = await WebAssembly.instantiate(bytes, imports);
  memory = instance.exports.memory;

  if (typeof instance.exports.main !== 'function') {
    console.error('module has no exported `main` function');
    process.exit(1);
  }

  instance.exports.main();
}

main().catch((err) => {
  console.error('error running wasm:', err);
  process.exit(1);
});
