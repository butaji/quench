const fs = require("fs");
const common = require("../common");

const source = "/tmp/quench-node-stage-87-source";
const destination = "/tmp/quench-node-stage-87-dest";
fs.writeFileSync(source, "copy");
fs.copyFile(
  source,
  destination,
  common.mustSucceed(() => {
    if (fs.readFileSync(destination, "utf8") !== "copy") {
      throw new Error("copyFile mismatch");
    }
  }),
);
try {
  fs.copyFileSync(false, destination);
  throw new Error("invalid source accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}
