// LSP client for tests/lsp_repl_w0001_gate.sh: open one file, print every
// publishDiagnostics. KRYOS_BIN = the kryos binary under test.
import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
const text = readFileSync(process.argv[2], 'utf8');
const p = spawn(process.env.KRYOS_BIN, ['lsp'], { stdio: ['pipe', 'pipe', 'pipe'] });
let buf = Buffer.alloc(0);
const msgs = [];
p.stdout.on('data', d => { buf = Buffer.concat([buf, d]);
  for (;;) { const i = buf.indexOf('\r\n\r\n'); if (i < 0) break;
    const n = +/Content-Length: (\d+)/i.exec(buf.subarray(0, i).toString())[1];
    if (buf.length < i + 4 + n) break;
    msgs.push(JSON.parse(buf.subarray(i + 4, i + 4 + n).toString('utf8')));
    buf = buf.subarray(i + 4 + n); } });
p.stderr.on('data', () => {});
const send = o => { const b = JSON.stringify(o); p.stdin.write(`Content-Length: ${Buffer.byteLength(b)}\r\n\r\n${b}`); };
const uri = 'file:///c:/tmp/t.kry';
send({jsonrpc:'2.0',id:1,method:'initialize',params:{processId:null,rootUri:null,capabilities:{}}});
send({jsonrpc:'2.0',method:'initialized',params:{}});
send({jsonrpc:'2.0',method:'textDocument/didOpen',params:{textDocument:{uri,languageId:'kryos',version:1,text}}});
setTimeout(() => { for (const m of msgs) if (m.method === 'textDocument/publishDiagnostics') console.log('publishDiagnostics:', JSON.stringify(m.params));
  p.kill(); process.exit(0); }, 2500);
