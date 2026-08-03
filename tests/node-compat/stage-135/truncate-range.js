const fs = require("fs");
const path = `/tmp/quench-node-stage-135-${process.pid}`;
fs.writeFileSync(path, "abc");
for (const value of [-1.5, 1.5]) {
  try {
    fs.truncateSync(path, value);
    throw new Error("accepted fractional length");
  } catch (error) {
    if (error.code !== "ERR_OUT_OF_RANGE") throw error;
  }
}
