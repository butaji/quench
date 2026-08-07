const crypto = require("crypto");

const hash = crypto.createHash("sha256").update("ff", "hex").digest("hex");
if (
  hash !== "a8100ae6aa1940d0b663bb31cd466142ebbdbd5187131b92d93818987832eb89"
) {
  throw new Error("hash update encoding was ignored");
}

const hmac = crypto
  .createHmac("sha256", "key")
  .update("ff", "hex")
  .digest("hex");
if (
  hmac !== "b666d53a97d509ef4185aeac4bedbaad03d6a600b74fb074400bff8586e69e4f"
) {
  throw new Error("hmac update encoding was ignored");
}

console.log("crypto update encoding passed");
