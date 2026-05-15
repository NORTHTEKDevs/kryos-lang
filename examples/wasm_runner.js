// Minimal WASM host runner for Kryos-emitted modules.
//
// Usage: node wasm_runner.js <path-to.wasm>
//
// Provides the three imports the WASM backend expects:
//   env.kryos_print_i64(i64)
//   env.kryos_print_f64(f64)
//   env.kryos_print_str(offset, len)
//
// Reads strings from the module's exported `memory`.

const fs = require('fs');

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
        // v is a BigInt in Node when the wasm import takes an i64.
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
