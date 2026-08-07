const assert = require("assert");
const cluster = require("cluster");
const http = require("http");

if (cluster.isWorker) {
  const server = new http.Server();
  server.listen(0, "127.0.0.1");
} else if (cluster.isPrimary) {
  const events = [];
  const worker = cluster.fork();
  cluster.on("disconnect", (w) => events.push({ source: "cluster", id: w.id }));
  worker.on(
    "disconnect",
    () => events.push({ source: "worker", id: worker.id }),
  );
  worker.on(
    "exit",
    (code, signal) =>
      events.push({ source: "exit", id: worker.id, code, signal }),
  );
  worker.on("listening", () => worker.disconnect());
  worker.on("exit", () => {
    assert.strictEqual(worker.exitedAfterDisconnect, true);
    assert.strictEqual(worker.state, "dead");
    assert.strictEqual(worker.process.exitCode, 0);
    assert.strictEqual(worker.process.signalCode, null);
    assert.deepStrictEqual(
      events.filter((e) => e.source === "cluster"),
      [{ source: "cluster", id: 1 }],
    );
    assert.deepStrictEqual(
      events.filter((e) => e.source === "worker"),
      [{ source: "worker", id: 1 }],
    );
    assert.deepStrictEqual(
      events.filter((e) => e.source === "exit"),
      [{ source: "exit", id: 1, code: 0, signal: null }],
    );
    console.log("cluster disconnect passed");
  });
}
