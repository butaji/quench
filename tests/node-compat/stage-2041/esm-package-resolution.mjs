import assert from "node:assert";
import { answer } from "fixture-package";

assert.strictEqual(answer, 42);
console.log("esm package resolution passed");
