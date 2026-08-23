// Minimal node:http2 loopback compatibility fixture.
const assert = require('node:assert');
const http2 = require('node:http2');
const http2Bare = require('http2');
assert.strictEqual(typeof http2.createServer, 'function');
assert.strictEqual(typeof http2.createSecureServer, 'function');
assert.strictEqual(typeof http2Bare.createServer, 'function');
assert.strictEqual(typeof http2Bare.createSecureServer, 'function');
assert.strictEqual(http2.HTTP2_HEADER_PATH, undefined);
assert.strictEqual(http2.constants.HTTP2_HEADER_PATH, ':path');
const server = http2.createServer((req, res) => {
  res.end('http2-loopback');
});
server.listen(0, () => {
  assert.strictEqual(typeof server.address().port, 'number');
  server.close();
});
console.log('http2 loopback ok');
