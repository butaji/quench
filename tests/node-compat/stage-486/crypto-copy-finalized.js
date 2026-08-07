const { createHash, createHmac } = require("crypto");

for (
  const context of [
    createHash("sha256").update("data"),
    createHmac("sha256", "key").update("data"),
  ]
) {
  context.digest();
  let error;
  try {
    context.copy();
  } catch (caught) {
    error = caught;
  }
  if (!error || error.code !== "ERR_CRYPTO_HASH_FINALIZED") {
    throw new Error("finalized crypto copy validation was missing");
  }
}

console.log("crypto finalized copy passed");
