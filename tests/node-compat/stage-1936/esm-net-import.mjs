import assert from "node:assert";
import { createServer, isIP } from "node:net";

assert.strictEqual(typeof createServer, "function");
assert.strictEqual(isIP("127.0.0.1"), 4);
console.log("esm net imports passed");
