const assert = require("assert");
const net = require("net");

const idle = net.connect({
  host: "localhost",
  port: 0,
  lookup() {},
});
assert.ok(idle);

const socket = net.connect({
  host: "localhost",
  port: 0,
  lookup(host, options, callback) {
    callback(null, "127.0.0.1", 100);
  },
});
socket.on("error", (error) => {
  assert.strictEqual(error.code, "ERR_INVALID_ADDRESS_FAMILY");
  assert.strictEqual(error.host, "localhost");
  assert.strictEqual(error.port, 0);
  assert.strictEqual(
    error.message,
    "Invalid address family: 100 localhost:0",
  );
  console.log("net lookup family passed");
});
