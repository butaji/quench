const fs = require("fs");
const path = `/tmp/quench-node-stage-415-${process.pid}`;

fs.writeFileSync(path, "descriptor data");
const fd = fs.openSync(path, "r");
const value = fs.readFileSync(fd, "utf8");
if (value !== "descriptor data") {
  throw new Error("readFileSync did not resolve a file descriptor");
}
fs.closeSync(fd);
fs.unlinkSync(path);

console.log("fs read file fd passed");
