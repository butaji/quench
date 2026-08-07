const assert = require("node:assert");
const fs = require("node:fs");

const controller = new AbortController();
fs.writeFile(
  "write-file-abort.txt",
  "data",
  { signal: controller.signal },
  (error) => {
    assert.strictEqual(error.name, "AbortError");
  },
);
controller.abort();
console.log("write file abort passed");
