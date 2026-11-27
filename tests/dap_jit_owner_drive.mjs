import { spawn } from 'node:child_process';
const K = process.env.KRYOS_BIN, prog = process.argv[2]; const PRE = process.env.KRYOS_PRE ? process.env.KRYOS_PRE.split(' ') : [];
const p = spawn(K, [...PRE, 'dap'], { stdio: ['pipe','pipe','pipe'], env: process.env });
let buf = Buffer.alloc(0); const out = []; let errs='';
p.stdout.on('data', d => { buf = Buffer.concat([buf,d]);
  for(;;){ const i = buf.indexOf('\r\n\r\n'); if(i<0)break; const m = /Content-Length: (\d+)/.exec(buf.subarray(0,i).toString()); const n=+m[1]; if(buf.length<i+4+n)break; out.push(JSON.parse(buf.subarray(i+4,i+4+n).toString('utf8'))); buf = buf.subarray(i+4+n);} });
p.stderr.on('data', d => errs += d);
let seq = 1; const send = (command, args={}) => { const b = JSON.stringify({seq:seq++,type:'request',command,arguments:args}); p.stdin.write(`Content-Length: ${Buffer.byteLength(b)}\r\n\r\n${b}`); };
send('initialize',{adapterID:'kryos'}); send('launch',{program:prog}); send('configurationDone');
const done = setTimeout(()=>finish('TIMEOUT'), 20000);
function finish(why){ clearTimeout(done);
  const lines = out.filter(m=>m.event==='output').map(m=>m.body.output).join('');
  const good = lines.split('\n').filter(l=>/^literal-\d\d-ok$/.test(l.trim())).length;
  const term = out.some(m=>m.event==='terminated');
  const launch = out.find(m=>m.command==='launch');
  console.log(JSON.stringify({why, launchOk: launch?.success, terminated: term, intactLiterals: good, of: 12, stderr: errs.slice(0,200), raw: lines.slice(0,160)}));
  p.kill(); process.exit(0); }
setInterval(()=>{ if(out.some(m=>m.event==='terminated')) finish('terminated'); }, 50);
