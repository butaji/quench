const assert = require("assert");
const { format } = require("util");

const value = {
  [Symbol.toPrimitive](hint) {
    return hint === "string" ? "string representation" : "default context";
  },
};

assert.strictEqual(format("%s", value), "string representation");
assert.strictEqual(format("%s", value + ""), "default context");
