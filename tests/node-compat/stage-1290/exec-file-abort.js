const assert = require("node:assert");
const { execFile } = require("node:child_process");

const controller = new AbortController();
execFile("runtime", [], { signal: controller.signal }, (error) => {
  assert.strictEqual(error.code, "ABORT_ERR");
  assert.strictEqual(error.name, "AbortError");
  assert.strictEqual(error.signal, undefined);
});
controller.abort();

console.log("execFile abort passed");
