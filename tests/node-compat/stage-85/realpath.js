const fs = require("fs");
const common = require("../common");

if (!common.canCreateSymLink()) process.exit(0);
const path = fs.realpathSync(".");
if (!path.startsWith("/")) throw new Error("realpath must be absolute");
fs.realpath(
  ".",
  common.mustSucceed((value) => {
    if (value !== path) throw new Error("async realpath mismatch");
  }),
);
