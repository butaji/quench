// http — real HTTP server over net: createServer, listen, and a raw
// net client exchanging one HTTP/1.1 request/response.
'use strict';
const http = require('node:http');
const net = require('node:net');
const assert = require('assert');

const server = http.createServer((req, res) => {
  assert.strictEqual(req.method, 'GET', 'request method');
  res.statusCode = 200;
  res.setHeader('Content-Type', 'text/plain');
  res.end('hello http');
});

server.on('error', () => {});
server.listen(0, '127.0.0.1', () => {
  const port = server.address().port;
  assert.ok(port > 0);

  const client = net.connect(port, '127.0.0.1', () => {
    client.write(
      'GET /path?q=1 HTTP/1.1\r\n' +
      'Host: localhost\r\n' +
      'Connection: close\r\n' +
      'X-Custom: yes\r\n' +
      '\r\n'
    );
  });
  let received = '';
  client.setEncoding('utf8');
  client.on('data', (chunk) => { received += chunk; });
  client.on('end', () => {
    assert.match(received, /^HTTP\/1\.1 200 OK\r\n/, 'status line');
    assert.match(received, /Content-Length: 10\r\n/, 'content length');
    assert.match(received, /Content-Type: text\/plain\r\n/, 'content type');
    assert.ok(received.endsWith('hello http'), 'body');
    server.close(() => {
      console.log('http: ok');
    });
  });
  client.on('close', () => {});
  client.on('error', () => {});
});