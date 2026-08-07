const assert = require("assert");
const common = require("../../tests/node/test/common");
const net = require("net");
const server = net.createServer();
server.listen(
  0,
  common.mustCall(function () {
    assert.strictEqual(this, server);
    assert.ok(this.address());
    console.log("net listen mustCall receiver passed");
  }),
);
