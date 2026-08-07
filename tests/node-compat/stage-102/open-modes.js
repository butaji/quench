const assert = require("assert");
const fs = require("fs");

const cases = [
  [0o640, 0o640],
  ["0640", 0o640],
  ["640", 0o640],
];

for (const [mode, expected] of cases) {
  const path = `/tmp/quench-node-stage-102-${String(mode)}`;
  const fd = fs.openSync(path, "w", mode);
  fs.closeSync(fd);
  assert.strictEqual(fs.statSync(path).mode & 0o777, expected);
  fs.rmSync(path);
}

const path = "/tmp/quench-node-stage-102-async";
fs.open(path, "w", 0o640, (error, fd) => {
  assert.ifError(error);
  fs.closeSync(fd);
  assert.strictEqual(fs.statSync(path).mode & 0o777, 0o640);
  fs.rmSync(path);
});

console.log("open modes passed");
