// node load.cjs SERVER_BIN SCENARIO TOTAL STEP CONCURRENCY : drives the framework request mix
// against a server process and samples its VmRSS/VmHWM every STEP requests.
const { spawn } = require('node:child_process'); const http = require('node:http'); const fs = require('node:fs');
const [bin, scenario, total, step, conc] = [process.argv[2], process.argv[3], +process.argv[4], +process.argv[5], +process.argv[6]];
const mix = [['GET', '/'], ['POST', '/echo', JSON.stringify({ a: 1, s: 'x'.repeat(2048) })], ['GET', '/stream'], ['GET', '/asset.txt'], ['GET', '/missing']];
const agent = new http.Agent({ keepAlive: true, maxSockets: conc });
const send = (port, [method, path, body]) => new Promise((res, rej) => {
  const req = http.request({ port, path, method, agent, headers: { 'accept-encoding': 'gzip', ...(body ? { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) } : {}) } },
    r => { r.resume(); r.on('end', () => res(r.statusCode)); });
  req.setTimeout(3000, () => { console.log('HANG', method, path); process.exit(3); }); req.on('error', rej); req.end(body);
});
const child = spawn(bin, [__dirname + '/serve.cjs', scenario], { cwd: require('node:path').dirname(scenario) + '/..', stdio: ['ignore', 'pipe', 'inherit'] });
const status = () => Object.fromEntries(fs.readFileSync(`/proc/${child.pid}/status`, 'utf8').split('\n').filter(l => /^(VmRSS|VmHWM|RssAnon|RssFile)/.test(l)).map(l => { const [k, v] = l.split(/:\s+/); return [k, (parseInt(v) / 1024).toFixed(1)]; }).concat([['cpu_ms', (() => { const f = fs.readFileSync(`/proc/${child.pid}/stat`, 'utf8').split(') ')[1].split(' '); return (+f[11] + +f[12]) * 10; })()]]));
child.stdout.on('data', async d => {
  const m = /PORT (\d+)/.exec(String(d)); if (!m) return; const port = +m[1];
  let done = 0, bad = 0; const t0 = Date.now();
  console.log(JSON.stringify({ requests: 0, ...status() }));
  const worker = async () => { while (done < total) { const i = done++; let s; try { s = await send(port, mix[i % mix.length]); } catch (e) { console.log('ERR at', i, mix[i % mix.length][1], e.code, e.message); process.exit(2); } if (s !== (i % 5 === 4 ? 404 : 200)) bad++; if ((i + 1) % step === 0) console.log(JSON.stringify({ requests: i + 1, ms: Date.now() - t0, bad, ...status() })); } };
  await Promise.all(Array.from({ length: conc }, worker));
  child.kill('SIGKILL'); agent.destroy();
});
