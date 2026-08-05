const assert = require("assert");
const dgram = require("dgram");
const { kStateSymbol } = require("internal/dgram");

const socket = dgram.createSocket("udp4");
assert.strictEqual(typeof socket[kStateSymbol].handle.fd, "number");
socket.close();
