"use strict";

const assert = require("assert");
const {
  transferableAbortSignal,
  transferableAbortController,
} = require("util");

const controller = new AbortController();
assert.strictEqual(
  transferableAbortSignal(controller.signal),
  controller.signal,
);
assert.throws(() => transferableAbortSignal({}), TypeError);
const transferable = transferableAbortController();
assert.strictEqual(transferable.signal.aborted, false);
transferable.abort();
assert.strictEqual(transferable.signal.aborted, true);

console.log("transferable abort passed");
