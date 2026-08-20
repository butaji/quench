// http — the request client round-trips against a local http server:
// http.request sends a request and reads the parsed response body.
'use strict';
const http = require('node:http');
const assert = require('assert');

const server = http.createServer((req, res) => {
  res.end('got ' + req.method + ' ' + req.url);
});
server.on('error', () => {});
server.listen(0, '127.0.0.1', () => {
  const port = server.address().port;

  const req = http.request(
    { host: '127.0.0.1', port, method: 'GET', path: '/a?b=1' },
    (res) => {
      assert.strictEqual(res.statusCode, 200, 'status code');
      let body = '';
      res.on('data', (chunk) => { body += chunk.toString(); });
      res.on('end', () => {
        assert.strictEqual(body, 'got GET /a?b=1', 'response body');
        server.close(() => console.log('http-client: ok'));
      });
    }
  );
  req.on('error', () => server.close());
  req.end();
});