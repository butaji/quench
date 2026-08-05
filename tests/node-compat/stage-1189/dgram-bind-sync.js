const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
const address = socket.bindSync({ address: "127.0.0.1", port: 0 });
assert.strictEqual(address.address, "127.0.0.1");
assert.strictEqual(address.family, "IPv4");
assert.ok(address.port > 0);
assert.deepStrictEqual(socket.address(), address);
socket.close();
