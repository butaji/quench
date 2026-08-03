const fs = require("fs");

const path = `/tmp/quench-node-stage-112-${process.pid}`;
fs.writeFileSync(path, "xyz\n");
const fd = fs.openSync(path, "r");
const buffer = Buffer.alloc(4);
if (
  fs.readSync(fd, buffer, { length: 4, position: 0 }) !== 4 ||
  buffer.toString() !== "xyz\n"
)
  throw new Error("options read mismatch");
fs.read(fd, { length: 4, position: 0 }, (error, count, result) => {
  if (error || count !== 4 || result.toString() !== "xyz\n")
    throw error || new Error("options async read mismatch");
  fs.closeSync(fd);
});
