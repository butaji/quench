const fs = require("fs");
const path = `/tmp/quench-node-stage-416-${process.pid}`;
let error;
try {
  fs.appendFile(path, false, () => {});
} catch (caught) {
  error = caught;
}
if (!error || error.code !== "ERR_INVALID_ARG_TYPE") {
  throw new Error("appendFile must validate data synchronously");
}

console.log("fs append validation passed");
