const assert = require("assert");
const cluster = require("cluster");
const http = require("http");

if (cluster.isWorker) {
  const server = http.createServer();
  server.listen(0, "127.0.0.1");
} else if (cluster.isPrimary) {
  const states = [];
  const clusterForked = [];
  const clusterOnline = [];
  const clusterListening = [];
  const clusterExit = [];

  cluster.on("fork", (w) => clusterForked.push({ id: w.id, state: w.state }));
  cluster.on("online", (w) => clusterOnline.push({ id: w.id, state: w.state }));
  cluster.on(
    "listening",
    (w) => clusterListening.push({ id: w.id, state: w.state }),
  );
  cluster.on("exit", (w) => clusterExit.push({ id: w.id, state: w.state }));

  const worker = cluster.fork();
  assert.strictEqual(worker.id, 1);
  assert.ok(
    worker instanceof cluster.Worker,
    "worker must be a cluster.Worker",
  );
  assert.strictEqual(typeof worker.send, "function");
  assert.strictEqual(typeof worker.kill, "function");
  assert.strictEqual(typeof worker.disconnect, "function");

  worker.on("listening", (info) => {
    assert.strictEqual(Object.keys(info).length, 4);
    assert.strictEqual(info.address, "127.0.0.1");
    assert.strictEqual(info.addressType, 4);
    assert.ok(Object.hasOwn(info, "fd"));
    assert.strictEqual(info.fd, undefined);
    assert.strictEqual(typeof info.port, "number");
    assert.ok(info.port >= 1 && info.port <= 65535);
    worker.kill();
  });
  worker.on("exit", (code, signal) => {
    assert.strictEqual(code, null);
    assert.strictEqual(signal, "SIGTERM");
    assert.strictEqual(worker.process.exitCode, null);
    assert.strictEqual(worker.process.signalCode, "SIGTERM");
    assert.deepStrictEqual(clusterForked, [{ id: 1, state: "none" }]);
    assert.deepStrictEqual(clusterOnline, [{ id: 1, state: "online" }]);
    assert.deepStrictEqual(clusterListening, [{ id: 1, state: "listening" }]);
    assert.deepStrictEqual(clusterExit, [{ id: 1, state: "dead" }]);
    console.log("cluster worker lifecycle passed");
  });
}
