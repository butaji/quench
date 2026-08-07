const assert = require("assert");
const original = new AbortController();
const combined = AbortSignal.any([original.signal]);
assert.strictEqual(combined.aborted, false);
original.abort("reason");
assert.strictEqual(combined.aborted, true);
assert.strictEqual(combined.reason, "reason");
assert.strictEqual(AbortSignal.any([]).aborted, false);
