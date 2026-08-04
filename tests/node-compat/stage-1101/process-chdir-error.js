const assert = require("node:assert");

assert.throws(() => process.chdir("does-not-exist"), {
  code: "ENOENT",
  syscall: "chdir",
  dest: "does-not-exist"
});
