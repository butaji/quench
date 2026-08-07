import path from "node:path";
import assert from "node:assert";

assert.strictEqual(path.basename("/tmp/entry.mjs"), "entry.mjs");
assert.strictEqual(typeof assert.strictEqual, "function");
console.log("esm builtin default import passed");
