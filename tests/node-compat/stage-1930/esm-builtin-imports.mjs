import assert from "node:assert";
import { cp, statSync } from "node:fs";
import { setTimeout } from "node:timers/promises";

assert.strictEqual(typeof cp, "function");
assert.strictEqual(typeof statSync, "function");
assert.strictEqual(await setTimeout(0, "ok"), "ok");
console.log("esm builtin imports passed");
