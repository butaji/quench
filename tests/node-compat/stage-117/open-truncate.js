const fs = require("fs");
const path = `/tmp/quench-node-stage-117-${process.pid}`;
fs.writeFileSync(path, "stale-data");
const fd = fs.openSync(path, "w");
fs.writeSync(fd, Buffer.from("fresh"));
fs.closeSync(fd);
if (fs.readFileSync(path).toString() !== "fresh")
  throw new Error("w flag did not truncate");
