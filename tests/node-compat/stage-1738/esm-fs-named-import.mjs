import { cp } from "node:fs";
import assert from "node:assert";
assert.strictEqual(typeof cp, "function");
assert.throws(() => cp("a", "b", "hello", () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});
console.log("esm fs named import passed");
