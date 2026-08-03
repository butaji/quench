const fs = require("fs");
const path = `/tmp/quench-node-stage-118-${process.pid}`;
fs.writeFileSync(path, "abcd");
const fd = fs.openSync(path, "w+");
fs.writevSync(fd, [Buffer.from("ab"), Buffer.from("cd")]);
fs.closeSync(fd);
if (!Buffer.from("abcd").equals(fs.readFileSync(path)))
  throw new Error("Buffer.equals mismatch");
