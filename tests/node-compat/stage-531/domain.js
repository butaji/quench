"use strict";

const assert = require("assert");
const domain = require("domain");

const current = domain.create();
let value = 0;
assert.strictEqual(current.add({}), current);
assert.strictEqual(
  current.run(() => ++value),
  1,
);
assert.strictEqual(current._active, false);
const bound = current.bind((left, right) => left + right);
assert.strictEqual(bound(2, 3), 5);
current.dispose();
assert.strictEqual(current.disposed, true);

console.log("domain passed");
