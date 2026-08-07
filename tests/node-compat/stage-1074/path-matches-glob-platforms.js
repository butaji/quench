const assert = require("assert");
const path = require("path");

assert.strictEqual(path.posix.matchesGlob("foo/bar/baz", "foo/**"), true);
assert.strictEqual(path.win32.matchesGlob("foo\\bar\\baz", "foo/**"), true);
assert.throws(() => path.matchesGlob(123, "foo/**"), {
  code: "ERR_INVALID_ARG_TYPE",
});
