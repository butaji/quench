const assert = require("assert");
const net = require("net");

const socket = net.connect(42, "not-a-real-host-" + "x".repeat(64));
socket.once("lookup", (error, address, family) => {
  assert(error instanceof Error);
  assert.strictEqual(error.code, "ENOTFOUND");
  assert.strictEqual(address, undefined);
  assert.strictEqual(family, undefined);
});
socket.once("error", (error) => {
  assert.strictEqual(error.code, "ENOTFOUND");
  console.log("net dns failure events passed");
});
