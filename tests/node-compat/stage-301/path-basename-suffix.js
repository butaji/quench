const assert = require("assert");
const path = require("path");

for (const namespace of [path.posix, path.win32]) {
  assert.throws(() => namespace.basename("file.txt", true), {
    code: "ERR_INVALID_ARG_TYPE",
  });
  assert.strictEqual(namespace.basename("file.txt", ".txt"), "file");
}
