const fs = require("fs");
const common = require("../common");

const path = "/tmp/quench-node-stage-102";
const fd = fs.openSync(path, "w+", "10644");
if ((fs.fstatSync(fd).mode & 0o777) !== 0o644) {
  throw new Error("open mode mismatch");
}
fs.closeSync(fd);
fs.open(
  path,
  "w+",
  0o600,
  common.mustSucceed((value) => fs.closeSync(value)),
);
