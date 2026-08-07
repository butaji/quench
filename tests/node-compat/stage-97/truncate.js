const fs = require("fs");
const common = require("../common");

const path = "/tmp/quench-node-stage-97";
fs.writeFileSync(path, "abcdef");
fs.truncateSync(path, 3);
if (fs.readFileSync(path, "utf8") !== "abc") {
  throw new Error("truncateSync mismatch");
}
fs.truncate(
  path,
  1,
  common.mustSucceed(() => {
    if (fs.readFileSync(path, "utf8") !== "a") {
      throw new Error("truncate mismatch");
    }
  }),
);
