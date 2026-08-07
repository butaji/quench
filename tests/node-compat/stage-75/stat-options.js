const fs = require("fs");
const common = require("../common");

const stats = fs.statSync(".", { bigint: false });
if (!(stats.mtime instanceof Date) || !stats.isDirectory()) {
  throw new Error("stat options mismatch");
}
if (
  fs.statSync("./quench-node-stage-75-missing", { throwIfNoEntry: false }) !==
    undefined
) {
  throw new Error("throwIfNoEntry mismatch");
}
fs.stat(
  ".",
  common.mustSucceed((value) => {
    if (!value.isDirectory()) throw new Error("async stat mismatch");
  }),
);
