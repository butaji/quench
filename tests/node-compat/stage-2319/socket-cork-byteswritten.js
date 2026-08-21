const assert = require("assert");
const net = require("net");

const socket = new net.Socket();
socket.cork();
socket.write("one");
socket.write(Buffer.from("twø", "utf8"));
assert.strictEqual(socket.bytesWritten, 7);
socket.uncork();
assert.strictEqual(socket.uncork(), socket);
console.log("socket cork bytesWritten passed");
