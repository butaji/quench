const fs = require("fs");

const path = `/tmp/quench-node-stage-115-${process.pid}`;
fs.writeFileSync(path, "abcd");
const fd = fs.openSync(path, "r");
const buffers = [Buffer.alloc(2), Buffer.alloc(2)];
if (
  fs.readvSync(fd, buffers, 0) !== 4 ||
  Buffer.concat(buffers).toString() !== "abcd"
)
  throw new Error("readv sync mismatch");
fs.closeSync(fd);
