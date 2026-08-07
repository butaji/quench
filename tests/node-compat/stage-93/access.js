const fs = require("fs");
const common = require("../common");

const path = "/tmp/quench-node-stage-93";
fs.writeFileSync(path, "access");
fs.accessSync(path, fs.constants.R_OK);
fs.access(
  path,
  fs.constants.R_OK,
  common.mustSucceed(() => {}),
);
fs.promises.access(path).then(common.mustCall());
