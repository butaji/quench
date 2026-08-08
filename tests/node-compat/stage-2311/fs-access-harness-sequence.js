const assert = require("assert");
const common = require("../../../tests/node/common");
const fs = require("fs");

const path = `${process.cwd()}/tests/node-compat/stage-2311/access.txt`;
try {
  fs.unlinkSync(path);
} catch (_) {}
fs.writeFileSync(path, "");
fs.chmodSync(path, 0o444);
process.setuid("nobody");

fs.access(
  path,
  fs.constants.W_OK,
  common.mustCall((error) => {
    assert.strictEqual(error.code, "EACCES");
  })
);
fs.promises
  .access(path, fs.constants.W_OK)
  .then(common.mustNotCall())
  .catch(
    common.mustCall((error) => {
      assert.strictEqual(error.code, "EACCES");
    })
  );
