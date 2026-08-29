"use strict";

const assert = require("assert");
const { Worker } = require("worker_threads");

const worker = new Worker(
  "const { parentPort } = require('worker_threads'); parentPort.postMessage({ ok: true });",
  { eval: true },
);
let received = false;
worker.on("message", (value) => {
  assert.deepStrictEqual(value, { ok: true });
  received = true;
  worker.terminate();
});
worker.on("exit", () => assert.strictEqual(received, true));
