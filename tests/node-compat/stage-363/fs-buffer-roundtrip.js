const assert = require("assert");
const fs = require("fs");
const path = `/tmp/quench-node-task-363-${process.pid}`;
const input = Buffer.from([0, 1, 2, 127, 128, 255]);
try {
  fs.writeFileSync(path, input);
  const output = fs.readFileSync(path);
  assert.deepStrictEqual(Array.from(output), Array.from(input));
} finally {
  try {
    fs.unlinkSync(path);
  } catch (_) {}
}
