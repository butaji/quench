"use strict";

const assert = require("assert");
const { Worker } = require("worker_threads");

const worker = new Worker("console.log('worker-stdout');", { eval: true });
assert.ok(worker.stdout);
assert.ok(worker.stderr);
let seen = false;
worker.stdout.once("data", (chunk) => {
  assert.match(String(chunk), /worker-stdout/);
  seen = true;
});
worker.on("exit", () => assert.strictEqual(seen, true));
