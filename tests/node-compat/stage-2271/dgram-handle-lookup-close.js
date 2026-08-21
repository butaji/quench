const assert = require("assert");
const dgram = require("dgram");
const { kStateSymbol } = require("internal/dgram");

const socket = dgram.createSocket("udp4");
const { handle } = socket[kStateSymbol];
const originalLookup = handle.lookup;
let called = false;
handle.lookup = function (address, callback) {
  called = true;
  socket.close(() => originalLookup.call(this, address, callback));
};
socket.bind(() => assert.fail("socket should not bind"));
setTimeout(() => {
  assert.strictEqual(called, true);
  console.log("dgram handle lookup close passed");
}, 10);
