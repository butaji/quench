const fs = require("fs");

let error;
try {
  fs.readFile("/tmp/does-not-matter");
} catch (caught) {
  error = caught;
}

if (!error || error.code !== "ERR_INVALID_ARG_TYPE") {
  throw new Error("readFile callback validation was missing");
}

console.log("fs readfile callback validation passed");
