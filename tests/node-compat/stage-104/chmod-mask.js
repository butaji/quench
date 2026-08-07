const fs = require("fs");

const path = "/tmp/quench-node-stage-104";
const fd = fs.openSync(path, "w+", "10644");
if ((fs.fstatSync(fd).mode & 0o777) !== 0o644) {
  throw new Error("open mode mask mismatch");
}
fs.fchmodSync(fd, 0o1600);
if ((fs.fstatSync(fd).mode & 0o777) !== 0o600) {
  throw new Error("fchmod mask mismatch");
}
fs.fchmodSync(fd, 0o1755);
if ((fs.fstatSync(fd).mode & 0o777) !== 0o755) {
  throw new Error("second fchmod mask mismatch");
}
fs.closeSync(fd);
