// HTTPS-compatible loopback surface (transport remains sole-runtime HTTP/1.1).
const assert = require('node:assert');
const https = require('node:https');
const net = require('node:net');
assert.strictEqual(typeof https.request, 'function');
assert.strictEqual(typeof https.get, 'function');
assert.strictEqual(typeof https.createServer, 'function');
assert.strictEqual(https.globalAgent.protocol, 'https:');
const server = https.createServer((req, res) => res.end('https-loopback'));
server.listen(0, '127.0.0.1', () => {
  const socket = net.connect(server.address().port, '127.0.0.1', () => {
    socket.write('GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n');
  });
  let body = '';
  socket.on('data', (chunk) => { body += chunk; });
  socket.on('end', () => {
    assert.ok(body.endsWith('https-loopback'));
    server.close(() => console.log('https: loopback ok'));
  });
  socket.on('error', (error) => { throw error; });
});
