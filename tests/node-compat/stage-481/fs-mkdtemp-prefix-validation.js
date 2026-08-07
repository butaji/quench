const fs = require("fs");

let error;
try {
  fs.mkdtempSync(123);
} catch (caught) {
  error = caught;
}

if (!error || error.code !== "ERR_INVALID_ARG_TYPE") {
  throw new Error("mkdtemp prefix validation was missing");
}

console.log("fs mkdtemp prefix validation passed");
