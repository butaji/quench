const fs = require("fs");
const assert = require("assert");

const path = `/tmp/quench-node-stage-131-${process.pid}`;
fs.writeFileSync(path, "a");
const fd = fs.openSync(path, "a");
fs.appendFile(fd, "b", (error) => {
  assert.ifError(error);
  fs.closeSync(fd);
  assert.strictEqual(fs.readFileSync(path, "utf8"), "ab");
  fs.rmSync(path);
});
