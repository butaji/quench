const fs = require("fs");
const assert = require("assert");

const target = `/tmp/quench-node-stage-108-target-${process.pid}`;
const link = `/tmp/quench-node-stage-108-link-${process.pid}`;
fs.writeFileSync(target, "x");
fs.symlinkSync(target, link);
assert.strictEqual(fs.statSync(link).isSymbolicLink(), false);
assert.strictEqual(fs.statSync(link).isFile(), true);
assert.strictEqual(fs.lstatSync(link).isSymbolicLink(), true);
assert.strictEqual(fs.lstatSync(link).isFile(), false);
fs.lstat(link, (error, stats) => {
  assert.ifError(error);
  assert.strictEqual(stats.isSymbolicLink(), true);
  fs.stat(link, (statError, followed) => {
    assert.ifError(statError);
    assert.strictEqual(followed.isFile(), true);
    fs.unlinkSync(link);
    fs.rmSync(target);
  });
});
