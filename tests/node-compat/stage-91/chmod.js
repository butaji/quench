const fs = require("fs");
const common = require("../common");

const path = "/tmp/quench-node-stage-91";
fs.writeFileSync(path, "");
fs.chmod(
  path,
  "644",
  common.mustSucceed(() => {
    if ((fs.statSync(path).mode & 0o777) !== 0o644) {
      throw new Error("chmod mode mismatch");
    }
  }),
);
