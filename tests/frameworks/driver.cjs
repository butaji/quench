// Usage: node driver.cjs scenarios/<framework>.cjs
// Serves the shared request set over loopback and checks framework behavior.
const http = require('node:http');
const requests = [
  ['GET', '/'], ['POST', '/echo', JSON.stringify({ a: 1, s: 'x'.repeat(2048) })],
  ['GET', '/stream'], ['GET', '/asset.txt'], ['GET', '/missing'],
];
const send = (port, [method, path, body, extraHeaders = {}]) => new Promise((res, rej) => {
  const req = http.request({ port, path, method, headers: { 'accept-encoding': 'gzip', ...extraHeaders, ...(body ? { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) } : {}) } }, r => {
    r.setEncoding('utf8');
    let responseBody = '';
    r.on('data', chunk => { responseBody += chunk; });
    r.on('end', () => res({ status: r.statusCode, headers: r.headers, body: responseBody }));
  });
  req.on('error', rej); req.end(body);
});
const check = (condition, message) => {
  if (!condition) throw new Error(`framework scenario failed: ${message}`);
};
(async () => {
  const app = require(require('node:path').resolve(process.argv[2]));
  const { port, stop } = await app.start();
  try {
    const responses = [];
    for (const request of requests) responses.push(await send(port, request));
    const statuses = responses.map(response => response.status);
    check(statuses.join(',') === '200,200,200,200,404', `statuses ${statuses.join(',')}`);
    const expectedBodies = [
      '<p>hi</p>',
      JSON.stringify({ a: 1, s: 'x'.repeat(2048) }),
      'abc',
      'static asset\n',
    ];
    for (const [index, expected] of expectedBodies.entries()) {
      check(responses[index].body === expected, `body for ${requests[index][1]}`);
    }

    if (process.argv[2].endsWith('express.cjs')) {
      const etag = responses[0].headers.etag;
      check(etag === 'W/"9-ttvLQjlZejsM8OHFMxIScRaHZZo"', `Express ETag ${etag}`);
      const conditional = await send(port, ['GET', '/', undefined, { 'if-none-match': etag }]);
      check(conditional.status === 304 && conditional.body === '', 'Express If-None-Match response');
    }
    console.log('statuses ' + statuses.join(','));
  } finally {
    await stop();
  }
})().catch(error => {
  console.log(error.stack || error);
  process.exitCode = 1;
});
