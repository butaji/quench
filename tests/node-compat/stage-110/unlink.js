const assert = require("assert");
const fs = require("fs");

for (const value of [false, 1, {}, [], null, undefined]) {
  assert.throws(() => fs.unlinkSync(value), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
  });
  assert.throws(() => fs.unlink(value, () => {}), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
  });
}

const path = `/tmp/quench-node-stage-110-${process.pid}`;
fs.writeFileSync(path, "x");
fs.unlink(path, (error) => {
  assert.ifError(error);
  assert.strictEqual(fs.existsSync(path), false);
  fs.writeFileSync(path, "x");
  fs.promises.unlink(path).then(() => {
    assert.strictEqual(fs.existsSync(path), false);
  });
});

console.log("unlink passed");
