// Minimal WASM host runner for Kryos-emitted modules.
//
// Usage: node wasm_runner.js <path-to.wasm>
//
// Provides the host imports the WASM backend expects:
//
//   ── v0.2: basic IO + simple containers ──
//   env.kryos_print_i64(i64)
//   env.kryos_print_f64(f64)
//   env.kryos_print_str(offset, len)
//   env.kryos_string_concat(off1, len1, off2, len2) -> i64
//   env.kryos_array_new(count) -> i64
//   env.kryos_array_get(packed, index) -> i64
//   env.kryos_array_set(packed, index, value)
//
//   ── v0.4: web-host stubs ──
//   env.kryos_dom_set_text(id_off, id_len, txt_off, txt_len)
//   env.kryos_dom_get_value(id_off, id_len) -> packed
//   env.kryos_alert(off, len)
//   env.kryos_canvas_fill_rect(id_off,id_len, x,y,w,h, c_off,c_len)
//   env.kryos_canvas_clear(id_off,id_len)
//   env.kryos_fetch_text(url_off, url_len) -> packed
//
//   ── v2.3: stdlib parity primitives ──
//   env.kryos_string_length(packed) -> i32
//   env.kryos_string_slice(packed, start, end) -> packed
//   env.kryos_string_to_upper(packed) -> packed
//   env.kryos_string_to_lower(packed) -> packed
//   env.kryos_string_trim(packed) -> packed
//   env.kryos_string_index_of(haystack, needle) -> i32  (-1 if absent)
//   env.kryos_string_parse_int(packed) -> i64
//   env.kryos_string_parse_float(packed) -> f64
//   env.kryos_array_length(packed) -> i32
//   env.kryos_array_push(packed, value) -> packed
//   env.kryos_array_pop(packed) -> i64                  (-1 if empty)
//   env.kryos_json_parse(packed_str) -> handle
//   env.kryos_json_stringify(handle) -> packed_str
//   env.kryos_json_get_int(handle, key_off, key_len) -> i64
//   env.kryos_json_get_str(handle, key_off, key_len) -> packed_str
//   env.kryos_regex_test(packed_pat, packed_subject) -> i32
//   env.kryos_regex_replace(packed_pat, packed_subject, packed_repl) -> packed
//   env.kryos_http_fetch(method_off,len, url_off,len, body_off,len) -> packed
//
// Reads/writes strings and arrays in the module's exported `memory`.

const fs = require('fs');

// Pack a 32-bit offset and a 32-bit length into one BigInt for the i64 return.
function pack(offset, len) {
  return (BigInt(len) << 32n) | (BigInt(offset) & 0xffffffffn);
}

// Unpack a packed-i64 handle into [offset, length] as plain Numbers.
function unpack(packed) {
  const bi = typeof packed === 'bigint' ? packed : BigInt(packed);
  return [Number(bi & 0xffffffffn), Number((bi >> 32n) & 0xffffffffn)];
}

// Bump-allocator state for host-allocated objects.
let heapPtr = 32 * 1024;

function bumpAlloc(memory, nBytes) {
  const needed = heapPtr + nBytes;
  const currentBytes = memory.buffer.byteLength;
  if (needed > currentBytes) {
    const extraPages = Math.ceil((needed - currentBytes) / 65536);
    memory.grow(extraPages);
  }
  const out = heapPtr;
  heapPtr += nBytes;
  heapPtr = (heapPtr + 7) & ~7;
  return out;
}

// JSON document handle table — index handles into parsed JS objects.
const jsonHandles = [];

async function main() {
  const wasmPath = process.argv[2];
  if (!wasmPath) {
    console.error('usage: node wasm_runner.js <file.wasm>');
    process.exit(1);
  }

  const bytes = fs.readFileSync(wasmPath);

  let memory = null;

  function readStr(off, len) {
    const o = typeof off === 'bigint' ? Number(off) : off;
    const l = typeof len === 'bigint' ? Number(len) : len;
    return new TextDecoder('utf-8').decode(new Uint8Array(memory.buffer, o, l));
  }

  function readPackedStr(packed) {
    const [o, l] = unpack(packed);
    return new TextDecoder('utf-8').decode(new Uint8Array(memory.buffer, o, l));
  }

  function writeStrToMem(s) {
    const enc = new TextEncoder().encode(s);
    const off = bumpAlloc(memory, enc.length);
    new Uint8Array(memory.buffer, off, enc.length).set(enc);
    return pack(off, enc.length);
  }

  function writeArrayToMem(values) {
    const offset = bumpAlloc(memory, values.length * 8);
    const dv = new DataView(memory.buffer);
    for (let i = 0; i < values.length; i++) {
      const v = typeof values[i] === 'bigint' ? values[i] : BigInt(values[i]);
      dv.setBigInt64(offset + i * 8, v, true);
    }
    return pack(offset, values.length);
  }

  function readArrayFromMem(packed) {
    const [offset, count] = unpack(packed);
    const dv = new DataView(memory.buffer);
    const out = new Array(count);
    for (let i = 0; i < count; i++) {
      out[i] = dv.getBigInt64(offset + i * 8, true);
    }
    return out;
  }

  const imports = {
    env: {
      // ── v0.2 ──
      kryos_print_i64(v) { console.log(v.toString()); },
      kryos_print_f64(v) { console.log(v); },
      kryos_print_str(offset, len) {
        if (!memory) {
          console.error('kryos_print_str called before memory available');
          return;
        }
        console.log(readStr(offset, len));
      },
      kryos_string_concat(off1, len1, off2, len2) {
        const totalLen = Number(len1) + Number(len2);
        const newOffset = bumpAlloc(memory, totalLen);
        const dst = new Uint8Array(memory.buffer);
        const src1 = new Uint8Array(memory.buffer, Number(off1), Number(len1));
        const src2 = new Uint8Array(memory.buffer, Number(off2), Number(len2));
        dst.set(src1, newOffset);
        dst.set(src2, newOffset + Number(len1));
        return pack(newOffset, totalLen);
      },
      kryos_array_new(count) {
        const c = Number(count);
        const bytes = c * 8;
        const offset = bumpAlloc(memory, bytes);
        new Uint8Array(memory.buffer, offset, bytes).fill(0);
        return pack(offset, c);
      },
      kryos_array_get(packed, index) {
        const [offset] = unpack(packed);
        const dv = new DataView(memory.buffer);
        return dv.getBigInt64(offset + Number(index) * 8, true);
      },
      kryos_array_set(packed, index, value) {
        const [offset] = unpack(packed);
        const dv = new DataView(memory.buffer);
        dv.setBigInt64(offset + Number(index) * 8, value, true);
      },

      // ── v0.4 web-host stubs (node fallback) ──
      kryos_dom_set_text(idOff, idLen, txtOff, txtLen) {
        console.error(`[dom_set_text] #${readStr(idOff, idLen)} <- ${readStr(txtOff, txtLen)}`);
      },
      kryos_dom_get_value(idOff, idLen) {
        console.error(`[dom_get_value] #${readStr(idOff, idLen)} -> (node stub: empty)`);
        return writeStrToMem('');
      },
      kryos_alert(off, len) {
        console.error(`[alert] ${readStr(off, len)}`);
      },
      kryos_canvas_fill_rect(idOff, idLen, x, y, w, h, cOff, cLen) {
        console.error(`[canvas_fill_rect] #${readStr(idOff, idLen)} (${x},${y}) ${w}x${h} ${readStr(cOff, cLen)}`);
      },
      kryos_canvas_clear(idOff, idLen) {
        console.error(`[canvas_clear] #${readStr(idOff, idLen)}`);
      },
      kryos_fetch_text(urlOff, urlLen) {
        console.error(`[fetch_text] ${readStr(urlOff, urlLen)} -> (node stub: empty)`);
        return writeStrToMem('');
      },

      // ── v2.3: stdlib parity primitives ──
      kryos_string_length(packed) {
        const [, len] = unpack(packed);
        return len;
      },
      kryos_string_slice(packed, start, end) {
        const s = readPackedStr(packed);
        const a = Number(start);
        const b = Number(end);
        return writeStrToMem(s.slice(a, b));
      },
      kryos_string_to_upper(packed) {
        return writeStrToMem(readPackedStr(packed).toUpperCase());
      },
      kryos_string_to_lower(packed) {
        return writeStrToMem(readPackedStr(packed).toLowerCase());
      },
      kryos_string_trim(packed) {
        return writeStrToMem(readPackedStr(packed).trim());
      },
      kryos_string_index_of(haystack, needle) {
        return readPackedStr(haystack).indexOf(readPackedStr(needle));
      },
      kryos_string_parse_int(packed) {
        const n = parseInt(readPackedStr(packed), 10);
        return Number.isFinite(n) ? BigInt(n) : 0n;
      },
      kryos_string_parse_float(packed) {
        const n = parseFloat(readPackedStr(packed));
        return Number.isFinite(n) ? n : 0;
      },
      kryos_array_length(packed) {
        const [, count] = unpack(packed);
        return count;
      },
      kryos_array_push(packed, value) {
        const values = readArrayFromMem(packed);
        values.push(typeof value === 'bigint' ? value : BigInt(value));
        return writeArrayToMem(values);
      },
      kryos_array_pop(packed) {
        const values = readArrayFromMem(packed);
        if (values.length === 0) return -1n;
        return values.pop();
      },
      kryos_json_parse(packed) {
        const txt = readPackedStr(packed);
        try {
          const obj = JSON.parse(txt);
          jsonHandles.push(obj);
          return BigInt(jsonHandles.length - 1);
        } catch (e) {
          jsonHandles.push(null);
          return -1n;
        }
      },
      kryos_json_stringify(handle) {
        const h = Number(handle);
        if (h < 0 || h >= jsonHandles.length) return writeStrToMem('');
        return writeStrToMem(JSON.stringify(jsonHandles[h]));
      },
      kryos_json_get_int(handle, keyOff, keyLen) {
        const h = Number(handle);
        if (h < 0 || h >= jsonHandles.length) return 0n;
        const obj = jsonHandles[h];
        if (obj === null || typeof obj !== 'object') return 0n;
        const v = obj[readStr(keyOff, keyLen)];
        if (typeof v === 'number') return BigInt(Math.trunc(v));
        if (typeof v === 'string') {
          const n = parseInt(v, 10);
          return Number.isFinite(n) ? BigInt(n) : 0n;
        }
        return 0n;
      },
      kryos_json_get_str(handle, keyOff, keyLen) {
        const h = Number(handle);
        if (h < 0 || h >= jsonHandles.length) return writeStrToMem('');
        const obj = jsonHandles[h];
        if (obj === null || typeof obj !== 'object') return writeStrToMem('');
        const v = obj[readStr(keyOff, keyLen)];
        if (v === undefined || v === null) return writeStrToMem('');
        return writeStrToMem(typeof v === 'string' ? v : JSON.stringify(v));
      },
      kryos_regex_test(packedPat, packedSubject) {
        try {
          const re = new RegExp(readPackedStr(packedPat));
          return re.test(readPackedStr(packedSubject)) ? 1 : 0;
        } catch (e) {
          return 0;
        }
      },
      kryos_regex_replace(packedPat, packedSubject, packedRepl) {
        try {
          const re = new RegExp(readPackedStr(packedPat), 'g');
          return writeStrToMem(readPackedStr(packedSubject).replace(re, readPackedStr(packedRepl)));
        } catch (e) {
          return writeStrToMem(readPackedStr(packedSubject));
        }
      },
      kryos_http_fetch(methodOff, methodLen, urlOff, urlLen, bodyOff, bodyLen) {
        const method = readStr(methodOff, methodLen);
        const url = readStr(urlOff, urlLen);
        const body = bodyLen > 0 ? readStr(bodyOff, bodyLen) : '';
        console.error(`[http_fetch] ${method} ${url} body=${body.length}B -> (node stub: empty)`);
        // Synchronous HTTP isn't viable in Node without --experimental-fetch sync
        // wrappers; we return an empty packed_str as the node-mode default. A
        // browser host can implement this via XMLHttpRequest in sync mode.
        return writeStrToMem('');
      },
    },
  };

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
