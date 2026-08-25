"use strict";

const assert = require("assert");
const { Worker } = require("worker_threads");

const worker = new Worker("setInterval(() => {}, 100);", { eval: true });
worker.unref();
worker.on("exit", () => assert.fail("an unref-only worker must not keep the parent alive"));
setTimeout(() => {}, 5);
