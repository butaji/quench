// http — the server streams a request body (Content-Length) to the
// request 'data'/'end', and the client sends/reads a POST payload.
'use strict';
const http = require('node:http');
const assert = require('assert');

const server = http.createServer((req, res) => {
  assert.strictEqual(req.method, 'POST', 'method');
  let body = '';
  req.on('data', (chunk) => { body += chunk.toString(); });
  req.on('end', () => {
    res.setHeader('Content-Type', 'text/plain');
    res.end('echo:' + body);
  });
});
server.on('error', () => {});
server.listen(0, '127.0.0.1', () => {
  const port = server.address().port;
  const req = http.request(
    { host: '127.0.0.1', port, method: 'POST', path: '/submit' },
    (res) => {
      let out = '';
      res.on('data', (chunk) => { out += chunk.toString(); });
      res.on('end', () => {
        assert.strictEqual(out, 'echo:abc', 'echoed request body');
        server.close(() => console.log('http-post: ok'));
      });
    }
  );
  req.on('error', () => server.close());
  req.end('abc');
});