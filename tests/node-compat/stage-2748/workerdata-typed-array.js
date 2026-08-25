"use strict";

const assert = require("assert");
const { Worker } = require("worker_threads");

const source = new Uint8Array([1, 2, 3, 4]);
const worker = new Worker(
  "require('worker_threads').parentPort.postMessage(require('worker_threads').workerData);",
  {
    eval: true,
    workerData: source,
    transferList: [source.buffer],
  },
);
assert.strictEqual(source.length, 0);
worker.once("message", (value) => assert.deepStrictEqual(value, new Uint8Array([1, 2, 3, 4])));
