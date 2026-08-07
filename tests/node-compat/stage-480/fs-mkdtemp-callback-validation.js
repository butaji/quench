const fs = require("fs");

let error;
try {
  fs.mkdtemp("/tmp/quench-node-");
} catch (caught) {
  error = caught;
}

if (!error || error.code !== "ERR_INVALID_ARG_TYPE") {
  throw new Error("mkdtemp callback validation was missing");
}

console.log("fs mkdtemp callback validation passed");
