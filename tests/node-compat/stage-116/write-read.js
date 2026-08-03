const fs = require("fs");

const path = `/tmp/quench-node-stage-116-${process.pid}`;
const fd = fs.openSync(path, "w+");
if (fs.writeSync(fd, Buffer.from("abcd")) !== 4)
  throw new Error("write mismatch");
const buffers = [Buffer.alloc(2), Buffer.alloc(2)];
if (
  fs.readvSync(fd, buffers, 0) !== 4 ||
  Buffer.concat(buffers).toString() !== "abcd"
)
  throw new Error("readv mismatch");
fs.closeSync(fd);
