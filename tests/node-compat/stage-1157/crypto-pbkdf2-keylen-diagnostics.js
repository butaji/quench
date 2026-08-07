const crypto = require("crypto");

try {
  crypto.pbkdf2Sync("password", "salt", 1, Infinity, "sha256");
  throw new Error("invalid key length was accepted");
} catch (error) {
  if (
    error.code !== "ERR_OUT_OF_RANGE" ||
    error.message !==
      'The value of "keylen" is out of range. It must be an integer. Received Infinity'
  ) {
    throw error;
  }
}
