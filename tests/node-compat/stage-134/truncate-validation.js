const fs = require("fs");
const path = `/tmp/quench-node-stage-134-${process.pid}`;
fs.writeFileSync(path, "abc");
for (const value of ["", false, null, {}, []]) {
  try {
    fs.truncate(path, value, () => {});
    throw new Error("accepted invalid length");
  } catch (error) {
    if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
  }
}
