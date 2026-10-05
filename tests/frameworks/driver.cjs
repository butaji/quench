// Usage: node driver.cjs scenarios/<framework>.cjs
// Serves the shared request set over loopback, prints response statuses to
// stderr and the builtins loaded while serving to stdout.
const base = new Set(process.moduleLoadList);
const http = require('node:http');
const requests = [
  ['GET', '/'], ['POST', '/echo', JSON.stringify({ a: 1, s: 'x'.repeat(2048) })],
  ['GET', '/stream'], ['GET', '/asset.txt'], ['GET', '/missing'],
];
const send = (port, [method, path, body]) => new Promise((res, rej) => {
  const req = http.request({ port, path, method, headers: { 'accept-encoding': 'gzip', ...(body ? { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) } : {}) } }, r => { r.resume(); r.on('end', () => res(r.statusCode)); });
  req.on('error', rej); req.end(body);
});
(async () => {
  const app = require(require('node:path').resolve(process.argv[2]));
  const { port, stop } = await app.start();
  const statuses = [];
  for (const r of requests) statuses.push(await send(port, r));
  await stop();
  const loaded = process.moduleLoadList.filter(m => !base.has(m) && m.startsWith('NativeModule ')).map(m => m.slice(13))
    .filter(m => !m.startsWith('internal/') || /internal\/(deps\/undici|webstreams|async_local|crypto\/webcrypto)/.test(m));
  process.stderr.write('statuses ' + statuses.join(',') + '\n');
  console.log(JSON.stringify(loaded.sort()));
  process.exit(0);
})();
