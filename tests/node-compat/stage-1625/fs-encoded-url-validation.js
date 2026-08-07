const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.readFile(new URL("file:///c:/tmp/%2f"), () => {}), {
  code: "ERR_INVALID_FILE_URL_PATH",
  name: "TypeError",
});
assert.throws(() => fs.readFile(new URL("file://hostname/a/b"), () => {}), {
  code: "ERR_INVALID_FILE_URL_HOST",
  name: "TypeError",
});
console.log("Filesystem encoded URL validation passed");
