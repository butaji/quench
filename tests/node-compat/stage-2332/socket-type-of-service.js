const assert = require("assert");
const net = require("net");

const socket = new net.Socket();
assert.strictEqual(socket.setTypeOfService(0x10), socket);
assert.strictEqual(socket.getTypeOfService(), 0x10);
assert.throws(() => socket.setTypeOfService("invalid"), {
  code: "ERR_INVALID_ARG_TYPE"
});
assert.throws(() => socket.setTypeOfService(256), {
  code: "ERR_OUT_OF_RANGE"
});
console.log("socket type of service passed");
