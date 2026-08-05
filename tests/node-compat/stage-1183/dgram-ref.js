const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
assert.strictEqual(socket.ref(), socket);
assert.strictEqual(socket.unref(), socket);
socket.close(() => socket.ref());
