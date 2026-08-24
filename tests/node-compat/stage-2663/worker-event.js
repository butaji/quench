"use strict";
const common = require("../../node/test/common");
const assert = require("assert");
const { Worker, threadId } = require("worker_threads");

process.on("worker", common.mustCall(({ threadId: createdThreadId }) => {
  assert.strictEqual(createdThreadId, threadId + 1);
}));

new Worker("", { eval: true });
