const assert = require("assert");

assert.throws(() => console.time(Symbol("label")), TypeError);
assert.throws(() => console.timeEnd(Symbol("label")), TypeError);
