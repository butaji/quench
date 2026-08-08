const assert = require("assert");
const fs = require("fs");
const path = `${process.cwd()}/tests/node-compat/stage-2309/access.txt`;

try {
  fs.unlinkSync(path);
} catch (_) {}
fs.writeFileSync(path, "");
fs.chmodSync(path, 0o444);
try {
  process.setuid("nobody");
} catch (_) {}
assert.throws(() => fs.accessSync(path, fs.constants.W_OK), {
  code: "EACCES"
});

fs.access(path, fs.constants.W_OK, (error) => {
  assert.strictEqual(error.code, "EACCES");
  console.log("fs access permissions passed");
});
