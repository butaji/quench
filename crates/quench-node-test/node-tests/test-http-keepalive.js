// http — keep-alive: two requests are served over one connection, framed
// by Content-Length, with Connection: keep-alive on each response.
'use strict';
const http = require('node:http');
const net = require('node:net');
const assert = require('assert');

const server = http.createServer((req, res) => res.end(req.url));
server.on('error', () => {});
server.listen(0, '127.0.0.1', () => {
  const port = server.address().port;

  const client = net.connect(port, '127.0.0.1', () => {
    client.write('GET /one HTTP/1.1\r\nHost: x\r\n\r\n');
  });
  let buf = '';
  const bodies = [];
  client.setEncoding('utf8');

  function consume() {
    const m = buf.match(
      /^HTTP\/1\.1 200 OK\r\n(?:[^\r\n]+\r\n)*Content-Length: (\d+)\r\n(?:[^\r\n]+\r\n)*\r\n/
    );
    if (!m) return;
    assert.match(m[0], /Connection: keep-alive\r\n/);
    const len = +m[1];
    const headLen = m[0].length;
    if (buf.length < headLen + len) return;
    bodies.push(buf.slice(headLen, headLen + len));
    buf = buf.slice(headLen + len);
    if (bodies.length === 1) {
      // Second request on the SAME connection.
      client.write('GET /two HTTP/1.1\r\nHost: x\r\n\r\n');
    } else if (bodies.length === 2) {
      assert.deepStrictEqual(bodies, ['/one', '/two'], 'two responses over one conn');
      client.end();
      server.close(() => console.log('http-keepalive: ok'));
    }
  }

  client.on('data', (chunk) => { buf += chunk; consume(); });
  client.on('error', () => {});
});
