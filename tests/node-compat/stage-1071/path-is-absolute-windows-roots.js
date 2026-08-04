const assert = require("assert");
const path = require("path");

for (const value of ["/", "//", "//server", "\\", "\\server\\file"]) {
  assert.strictEqual(path.win32.isAbsolute(value), true);
}
for (const value of ["c", "c:", "C:cwd\\another"]) {
  assert.strictEqual(path.win32.isAbsolute(value), false);
}
