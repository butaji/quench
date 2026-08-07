const fs = require("fs");
const common = require("../common");

const stats = fs.statfsSync(".", { bigint: true });
if (typeof stats.blocks !== "bigint") throw new Error("statfs bigint mismatch");
fs.statfs(
  ".",
  common.mustSucceed((value) => {
    if (typeof value.blocks !== "number") throw new Error("statfs mismatch");
  }),
);
