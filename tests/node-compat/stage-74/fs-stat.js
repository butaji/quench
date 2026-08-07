const fs = require("fs");
const common = require("../common");

fs.stat(
  ".",
  common.mustSucceed((stats) => {
    if (!(stats.mtime instanceof Date) || !stats.isDirectory()) {
      throw new Error("stat metadata mismatch");
    }
  }),
);
fs.lstat(
  ".",
  common.mustSucceed((stats) => {
    if (!(stats.mtime instanceof Date)) {
      throw new Error("lstat metadata mismatch");
    }
  }),
);
const fd = fs.openSync(".", "r");
if (!fs.fstatSync(fd).isDirectory()) throw new Error("fstat metadata mismatch");
fs.close(fd, common.mustSucceed());
