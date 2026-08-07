import assert from "node:assert";
import { glob, globSync } from "node:fs/promises";

assert.strictEqual(typeof glob, "function");
assert.strictEqual(typeof globSync, "function");
console.log("fs glob ESM surface passed");
