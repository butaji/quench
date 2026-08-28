"use strict";

const assert = require("assert");
const domain = require("domain");

const outer = domain.create();
const inner = domain.create();
let observed;

outer.run(() => {
  inner.run(() => {
    assert.strictEqual(domain._stack.length, 2);
    process.nextTick(() => {
      observed = [domain._stack.length, process.domain];
    });
  });
});

assert.strictEqual(domain._stack.length, 0);
assert.strictEqual(process.domain, undefined);
setImmediate(() => {
  assert.strictEqual(observed[0], 2);
  assert.strictEqual(observed[1], inner);
});
