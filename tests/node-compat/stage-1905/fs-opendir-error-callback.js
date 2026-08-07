const assert = require("assert");
const fs = require("fs");

(async () => {
  const error = await new Promise((resolve) => {
    fs.opendir(__filename, (value) => resolve(value));
  });
  assert.strictEqual(error.code, "ENOTDIR");
})();
