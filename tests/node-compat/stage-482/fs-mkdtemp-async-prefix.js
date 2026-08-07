const fs = require("fs");

let error;
try {
  fs.mkdtemp(123, () => {});
} catch (caught) {
  error = caught;
}

if (!error || error.code !== "ERR_INVALID_ARG_TYPE") {
  throw new Error("async mkdtemp prefix validation was missing");
}

console.log("fs async mkdtemp prefix passed");
