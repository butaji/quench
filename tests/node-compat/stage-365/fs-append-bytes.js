const assert = require("assert");
const fs = require("fs");
const path = `/tmp/quench-node-task-365-${process.pid}`;
try {
  fs.writeFileSync(path, Buffer.from([1, 2]));
  fs.writeFileSync(path, Buffer.from([3, 4]), { flag: "a" });
  assert.deepStrictEqual(Array.from(fs.readFileSync(path)), [1, 2, 3, 4]);
} finally {
  try {
    fs.unlinkSync(path);
  } catch (_) {}
}
