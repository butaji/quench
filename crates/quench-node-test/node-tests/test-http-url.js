// http — the client accepts a string URL: http.get('http://host:port/...')
// routes to a local server and parses the response.
'use strict';
const http = require('node:http');
const assert = require('assert');

const server = http.createServer((req, res) => res.end('ok:' + req.url));
server.on('error', () => {});
server.listen(0, '127.0.0.1', () => {
  const port = server.address().port;
  http.get('http://127.0.0.1:' + port + '/ping?q=1', (res) => {
    assert.strictEqual(res.statusCode, 200, 'status');
    let body = '';
    res.on('data', (chunk) => { body += chunk.toString(); });
    res.on('end', () => {
      assert.strictEqual(body, 'ok:/ping?q=1', 'url path routed');
      server.close(() => console.log('http-url: ok'));
    });
  });
});