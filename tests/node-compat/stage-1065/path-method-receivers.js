const assert = require("assert");
const path = require("path");

for (const namespace of [path.posix, path.win32]) {
  assert.strictEqual(typeof namespace.resolve, "function");
  assert.throws(() => namespace.join.call(null, 1), {
    code: "ERR_INVALID_ARG_TYPE",
  });
  assert.throws(() => namespace.resolve(1), {
    code: "ERR_INVALID_ARG_TYPE",
  });
  assert.throws(() => namespace.relative.call(null, 1, "x"), {
    code: "ERR_INVALID_ARG_TYPE",
  });
  assert.strictEqual(namespace.extname.call(null, "file.txt"), ".txt");
}
