import assert from "assert";
import {
  createRequire,
  getPort,
  hasCrypto,
  hasQuic,
  mustCall,
  platformTimeout,
  skipIfInspectorDisabled,
  spawnPromisified,
} from "../../../tests/node/test/common/index.mjs";

assert.strictEqual(typeof createRequire, "function");
assert.strictEqual(typeof getPort, "function");
assert.strictEqual(typeof hasCrypto, "boolean");
assert.strictEqual(typeof hasQuic, "boolean");
assert.strictEqual(typeof mustCall, "function");
assert.strictEqual(typeof platformTimeout, "function");
assert.strictEqual(typeof platformTimeout(1), "number");
assert.strictEqual(typeof skipIfInspectorDisabled, "function");
assert.strictEqual(typeof spawnPromisified, "function");
console.log("esm common index exports passed");
