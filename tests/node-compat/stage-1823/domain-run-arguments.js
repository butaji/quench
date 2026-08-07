const assert = require("assert");
const domain = require("domain");

const d = new domain.Domain();
assert.strictEqual(
  d.run((a, b) => `${a} ${b}`, "return", "value"),
  "return value",
);

console.log("domain run arguments passed");
