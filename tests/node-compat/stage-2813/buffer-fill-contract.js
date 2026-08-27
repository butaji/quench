const assert = require("assert");
const { Buffer } = require("buffer");

const value = Buffer.from("abcdef");
assert.strictEqual(value.fill("x", 1, 4), value);
assert.strictEqual(value.toString(), "axxxef");
assert.strictEqual(Buffer.from("abc").fill(65).toString(), "AAA");
assert.throws(() => Buffer.from("abc").fill("x", -1), { code: "ERR_OUT_OF_RANGE" });
console.log("buffer fill contract pass");
