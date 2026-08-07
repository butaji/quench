const assert = require("assert");
const { Worker } = require("worker_threads");

const worker = new Worker("unused");
assert.strictEqual(
  worker.on("message", () => {}),
  worker,
);
assert.strictEqual(
  worker.once("exit", () => {}),
  worker,
);
worker.postMessage("value");
worker.terminate();
