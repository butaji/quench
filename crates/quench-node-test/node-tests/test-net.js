// net — pure isIP helpers plus a real loopback TCP round-trip:
// server.listen on an ephemeral port, connect, write/read echo, close.
'use strict';
const net = require('node:net');
const assert = require('assert');

// Static IP helpers.
assert.strictEqual(net.isIP('127.0.0.1'), 4);
assert.strictEqual(net.isIP('::1'), 6);
assert.strictEqual(net.isIP('not-an-ip'), 0);
assert.strictEqual(net.isIPv4('127.0.0.1'), true);
assert.strictEqual(net.isIPv4('::1'), false);
assert.strictEqual(net.isIPv6('::1'), true);

// Real loopback TCP: server.listen, socket write/read round-trip.
const server = net.createServer((socket) => {
  assert.strictEqual(socket.remoteAddress, '127.0.0.1');
  socket.on('data', (chunk) => socket.write(chunk)); // echo
  socket.on('end', () => socket.end());
  socket.on('error', () => {});
});

server.on('error', () => {});
server.listen(0, '127.0.0.1', () => {
  const port = server.address().port;
  assert.ok(port > 0, 'ephemeral port assigned');

  const client = net.connect(port, '127.0.0.1', () => {
    client.write('hello net');
  });
  const chunks = [];
  client.setEncoding('utf8');
  client.on('data', (chunk) => {
    chunks.push(chunk);
    client.end();
    server.close(() => {
      assert.strictEqual(chunks.join(''), 'hello net', 'echoed payload');
      console.log('net: ok');
    });
  });
  client.on('error', () => {});
  client.on('close', () => {});
});