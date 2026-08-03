const fs = require("fs");

const path = `/tmp/quench-node-stage-131-${process.pid}`;
fs.writeFileSync(path, "a");
const fd = fs.openSync(path, "a");
fs.appendFile(fd, "b", (error) => {
  if (error) throw error;
  fs.closeSync(fd);
  if (fs.readFileSync(path, "utf8") !== "ab")
    throw new Error("append fd mismatch");
});
