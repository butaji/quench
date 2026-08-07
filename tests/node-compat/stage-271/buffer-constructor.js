const assert = require("assert");
const { Buffer } = require("buffer");

assert.strictEqual(new Buffer(4).length, 4);
const value = new Buffer("ok");
if (value.toString() !== "ok") {
  throw new Error("string legacy constructor failed");
}
