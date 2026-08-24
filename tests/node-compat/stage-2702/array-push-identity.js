const assert = require("assert");

const first = { name: "first" };
const second = { name: "second" };
const values = [];
values.push(first);
values.push(second);

assert.strictEqual(values.length, 2);
assert.strictEqual(values[0], first);
assert.strictEqual(values[1], second);
assert.strictEqual(values.indexOf(second), 1);
