const fs = require("fs");

const path = `/tmp/quench-node-stage-111-${process.pid}`;
fs.writeFileSync(path, "xyz\n");
const fd = fs.openSync(path, "r");
const buffer = Buffer.alloc(4);
if (fs.readSync(fd, buffer, 0, 4, 0) !== 4 || buffer.toString() !== "xyz\n") {
  throw new Error("sync read mismatch");
}
fs.read(fd, Buffer.alloc(0), 0, 0, 0, (error, count) => {
  if (error || count !== 0) throw error || new Error("async read mismatch");
  fs.closeSync(fd);
});
