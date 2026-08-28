const assert = require("assert");
const cluster = require("cluster");
const net = require("net");

if (cluster.isPrimary) {
  const worker = cluster.fork();
  worker.once("listening", (address) => {
    const client = net.connect(address.port, address.address, () => {
      assert.strictEqual(worker.send("from-parent"), true);
    });
    client.on("data", (data) => {
      assert.strictEqual(data.toString(), "from-child");
      client.end();
    });
    client.on("end", () => worker.kill());
  });
  worker.once("message", (message) => assert.strictEqual(message, "from-child"));
  worker.once("exit", (code) => {
    assert.strictEqual(code, 0);
    process.exit(0);
  });
} else {
  const server = net.createServer((socket) => {
    process.send("from-child");
    process.on("message", (message) => {
      assert.strictEqual(message, "from-parent");
      socket.end("from-child");
    });
  });
  server.listen(0, "127.0.0.1");
}
