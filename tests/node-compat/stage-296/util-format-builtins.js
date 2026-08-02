const assert = require("assert");
const util = require("util");

const date = new Date("2023-10-01T00:00:00Z");
assert.strictEqual(util.format("%s", date), util.inspect(date));
assert.strictEqual(util.inspect(Symbol("foo")), "Symbol(foo)");
assert.strictEqual(util.format("%s", Symbol("foo")), "Symbol(foo)");
