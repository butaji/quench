const assert = require("assert");
const net = require("net");
const server = net.createServer();
server.listen(0, function () {
  assert.strictEqual(this, server);
  assert.strictEqual(typeof this.address, "function");
  assert.strictEqual(this.address().port, 0);
  console.log("net listen callback receiver passed");
});
