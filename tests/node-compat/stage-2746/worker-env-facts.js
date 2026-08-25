"use strict";

const assert = require("assert");
const { Worker } = require("worker_threads");

assert.throws(() => new Worker("", { eval: true, env: 42 }), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_TYPE",
});

const worker = new Worker(
  "require('worker_threads').parentPort.postMessage(process.env.WORKER_ONLY);",
  { eval: true, env: { WORKER_ONLY: "yes" } },
);
worker.once("message", (value) => assert.strictEqual(value, "yes"));
