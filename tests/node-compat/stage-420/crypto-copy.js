const crypto = require("crypto");

const hash = crypto.createHash("sha256").update("prefix");
const hashCopy = hash.copy();
if (
  hash.update("-one").digest("hex") === hashCopy.update("-two").digest("hex")
) {
  throw new Error("hash.copy did not branch state");
}

const hmac = crypto.createHmac("sha256", "key").update("prefix");
const hmacCopy = hmac.copy();
if (
  hmac.update("-one").digest("hex") === hmacCopy.update("-two").digest("hex")
) {
  throw new Error("hmac.copy did not branch state");
}

console.log("crypto copy passed");
