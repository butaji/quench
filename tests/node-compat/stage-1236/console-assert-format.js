const assert = require("assert");

const originalWrite = process.stderr.write;
let output = "";
process.stderr.write = (value) => {
  output += value;
  return true;
};
console.assert(false, "%s should", "console.assert", "not throw");
process.stderr.write = originalWrite;
assert.strictEqual(
  output,
  "Assertion failed: console.assert should not throw\n",
);
