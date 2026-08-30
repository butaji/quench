const assert = require("assert");
const cluster = require("cluster");
const net = require("net");

if (cluster.isPrimary) {
  cluster.fork().once("exit", (code) => {
    assert.strictEqual(code, 0);
    process.exit(0);
  });
} else {
  const server = net.createServer(() => assert.fail("connection"));
  assert.strictEqual(server.listen(process.exit), server);
}
