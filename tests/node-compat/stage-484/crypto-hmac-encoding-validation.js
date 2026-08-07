const { createHmac } = require("crypto");

let error;
try {
  createHmac("sha256", "key").update("data").digest("not-an-encoding");
} catch (caught) {
  error = caught;
}

if (!error || error.code !== "ERR_UNKNOWN_ENCODING") {
  throw new Error("HMAC digest encoding validation was missing");
}

console.log("crypto hmac encoding validation passed");
