"use strict";
const assert = require("assert");
const { Worker } = require("worker_threads");
const worker = new Worker("require('worker_threads').parentPort.postMessage('ok');", { eval: true });
worker.once("message", (value) => assert.strictEqual(value, "ok"));
