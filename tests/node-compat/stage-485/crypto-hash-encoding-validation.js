const { createHash } = require("crypto");

let error;
try {
  createHash("sha256").update("data").digest("not-an-encoding");
} catch (caught) {
  error = caught;
}

if (!error || error.code !== "ERR_UNKNOWN_ENCODING") {
  throw new Error("hash digest encoding validation was missing");
}

console.log("crypto hash encoding validation passed");
