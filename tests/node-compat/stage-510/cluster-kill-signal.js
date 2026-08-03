const assert = require("assert");
const cluster = require("cluster");
const http = require("http");

if (cluster.isWorker) {
  const server = new http.Server();
  server.listen(0, "127.0.0.1");
} else if (cluster.isPrimary) {
  const worker = cluster.fork();
  let disconnectFired = false;
  worker.on("disconnect", () => {
    disconnectFired = true;
  });
  worker.on("exit", (code, signal) => {
    assert.strictEqual(code, null);
    assert.strictEqual(signal, "SIGKILL");
    assert.strictEqual(disconnectFired, false);
    assert.strictEqual(worker.exitedAfterDisconnect, false);
    assert.strictEqual(worker.state, "dead");
    assert.strictEqual(worker.process.exitCode, null);
    assert.strictEqual(worker.process.signalCode, "SIGKILL");
    console.log("cluster kill signal passed");
  });
  worker.on("listening", () => worker.kill("SIGKILL"));
}
