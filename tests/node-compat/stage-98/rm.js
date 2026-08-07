const fs = require("fs");
const common = require("../common");

const path = "/tmp/quench-node-stage-98";
fs.mkdirSync(path);
fs.rm(
  path,
  common.mustSucceed(() => {
    if (fs.existsSync(path)) throw new Error("rm did not remove path");
  }),
);
