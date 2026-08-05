const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.connectSync(12345, "127.0.0.1");
assert.strictEqual(socket.remoteAddress().address, "127.0.0.1");
assert.strictEqual(socket.remoteAddress().port, 12345);
assert.ok(socket.address().port > 0);
socket.close();
