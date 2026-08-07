const crypto = require("crypto");

const hash = crypto.createHash("sha256").update("abc").digest("base64");
if (hash !== "ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=") {
  throw new Error("sha256 base64 digest failed");
}

const hmac = crypto.createHmac("sha256", "key").update("abc").digest("base64");
if (hmac !== "nBluMtwBdfhvSxy4konWYZ3mvuaZ5MN45oMJ7Zehpqs=") {
  throw new Error("hmac base64 digest failed");
}

console.log("crypto base64 digest passed");
