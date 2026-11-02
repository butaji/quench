// Usage: <node-or-quench> parity.cjs scenarios/<framework>.cjs
// Prints one normalized line per response so two binaries' outputs can be diffed:
//   diff <(node parity.cjs S) <(quench-node parity.cjs S)
// Run-specific values (timestamps, Next build IDs) are removed before the length is taken.
const http = require('node:http');
const path = require('node:path');

const requests = [
  ['GET', '/'], ['POST', '/echo', JSON.stringify({ a: 1, s: 'x'.repeat(2048) })],
  ['GET', '/stream'], ['GET', '/asset.txt'], ['GET', '/missing'],
];
const TIMESTAMP = /[0-9]{10,}/g;
const BUILD_ID = /"buildId":"[^"]*"/g;
const normalizedLength = body => body.replace(TIMESTAMP, 'T').replace(BUILD_ID, '').length;

const send = (port, [method, route, body]) => new Promise((resolve, reject) => {
  const headers = { 'accept-encoding': 'gzip' };
  if (body) Object.assign(headers, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) });
  const request = http.request({ port, path: route, method, headers }, response => {
    let received = '';
    response.setEncoding('latin1');
    response.on('data', chunk => { received += chunk; });
    response.on('end', () => {
      const type = (response.headers['content-type'] || '').split(';')[0];
      console.log(method, route, response.statusCode, type, response.headers['content-encoding'] || '-', normalizedLength(received));
      resolve();
    });
  });
  request.on('error', reject);
  request.end(body);
});

(async () => {
  const { port, stop } = await require(path.resolve(process.argv[2])).start();
  for (const request of requests) await send(port, request);
  await stop();
})().catch(error => {
  console.log('ERROR', (error && error.stack) || error);
  process.exitCode = 1;
});
