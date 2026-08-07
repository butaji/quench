const fs = require("fs");
const common = require("../common");

const path = "/tmp/quench-node-stage-92";
const fd = fs.openSync(path, "w");
fs.fchmod(
  fd,
  0o600,
  common.mustSucceed(() => {
    if ((fs.statSync(path).mode & 0o777) !== 0o600) {
      throw new Error("fchmod mismatch");
    }
  }),
);
