const assert = require("assert");
const fs = require("fs");
const path = require("path");

const file = path.join(process.cwd(), "tests/node/test/.tmp.0/access-mode");
fs.writeFileSync(file, "");
fs.chmodSync(file, 0o444);

assert.doesNotReject(fs.promises.access(file, fs.constants.R_OK));
assert.rejects(fs.promises.access(path.join(file, "missing")), (error) => {
  assert.strictEqual(error.code, "ENOENT");
  return true;
});
