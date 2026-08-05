const assert = require("assert");
const net = require("net");
const tls = require("tls");

assert.strictEqual(Object.getPrototypeOf(tls.TLSSocket), net.Socket);
assert.strictEqual(tls.TLSSocket.prototype.bytesWritten, undefined);
