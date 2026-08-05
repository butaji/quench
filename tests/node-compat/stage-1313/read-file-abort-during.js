const assert = require("node:assert");
const fs = require("node:fs");

const controller = new AbortController();
fs.readFile(__filename, { signal: controller.signal }, (error) => {
  assert.strictEqual(error.name, "AbortError");
});
process.nextTick(() => controller.abort());
console.log("read file abort passed");
