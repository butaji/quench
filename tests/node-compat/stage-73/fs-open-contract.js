const fs = require("fs");
const common = require("../common");

const path = "/tmp/quench-node-stage-73-file";
fs.closeSync(fs.openSync(path, "w"));
fs.open(
  path,
  "r",
  common.mustSucceed((fd) => fs.closeSync(fd)),
);
if (!fs.openSync(path, "r")) throw new Error("openSync failed");
try {
  fs.openSync("/tmp/quench-node-stage-73-missing", "r");
  throw new Error("missing open succeeded");
} catch (error) {
  if (error.code !== "ENOENT") throw error;
}
try {
  fs.open(false, "r", common.mustNotCall());
  throw new Error("invalid open succeeded");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}
