const assert = require("assert");
const net = require("net");

const socket = new net.Socket();
assert.strictEqual(socket.bytesRead, 0);
assert.strictEqual(socket.bytesWritten, 0);
socket.write("hello");
assert.strictEqual(socket.bytesWritten, 5);
socket.write(Buffer.alloc(7));
assert.strictEqual(socket.bytesWritten, 12);
assert.strictEqual(socket.bytesRead, 0);
socket.end();
