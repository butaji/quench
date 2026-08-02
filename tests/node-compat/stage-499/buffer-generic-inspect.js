const assert = require("assert");

const inspect = Buffer.prototype[Symbol.for("nodejs.util.inspect.custom")];
assert.strictEqual(inspect.call(new Uint8Array([1, 2])), "<Uint8Array 01 02>");
