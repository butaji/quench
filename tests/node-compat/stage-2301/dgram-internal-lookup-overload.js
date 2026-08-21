const assert = require("assert");
const dgram = require("dgram");
const { kStateSymbol } = require("internal/dgram");

const socket = dgram.createSocket("udp4");
const handle = socket[kStateSymbol].handle;
const originalLookup = handle.lookup;
let argumentsSeen;
handle.lookup = function (address, callback) {
  argumentsSeen = arguments.length;
  socket.close(() => originalLookup.call(this, address, callback));
};
socket.bind(() => assert.fail("bind callback must not run"));
setImmediate(() => {
  assert.strictEqual(argumentsSeen, 2);
  console.log("dgram internal lookup overload passed");
});
