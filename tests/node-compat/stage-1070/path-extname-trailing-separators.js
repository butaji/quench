const assert = require("assert");
const path = require("path");

for (const namespace of [path.posix, path.win32]) {
  assert.strictEqual(namespace.extname(".."), "");
  assert.strictEqual(namespace.extname("file.ext/"), ".ext");
  assert.strictEqual(namespace.extname("file."), ".");
}

assert.strictEqual(path.win32.extname("C:file.ext\\"), ".ext");
