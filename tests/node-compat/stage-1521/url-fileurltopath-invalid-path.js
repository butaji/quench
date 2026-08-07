const assert = require("node:assert");
const url = require("node:url");

assert.throws(
  () => url.fileURLToPath("file:///a%2F/"),
  (error) => {
    assert.strictEqual(error.code, "ERR_INVALID_FILE_URL_PATH");
    assert.strictEqual(error.input.href, "file:///a%2F/");
    return true;
  },
);
console.log("url fileURLToPath invalid path passed");
