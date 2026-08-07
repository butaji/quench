const fs = require("fs");
const path = `/tmp/quench-node-stage-419-${process.pid}`;
let error;
try {
  fs.statSync(path);
} catch (caught) {
  error = caught;
}
if (!error || error.code !== "ENOENT" || error.syscall !== "stat") {
  throw new Error("statSync must preserve ENOENT metadata");
}

console.log("fs stat enoent passed");
