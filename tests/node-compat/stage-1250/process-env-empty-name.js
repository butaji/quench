const assert = require("node:assert");

process.env[""] = "ignored";
assert.strictEqual(process.env[""], undefined);
assert.strictEqual(Object.hasOwn(process.env, ""), false);

console.log("process.env empty name passed");
