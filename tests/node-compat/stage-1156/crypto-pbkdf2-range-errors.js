const crypto = require("crypto");

for (const iterations of [-1, 0, 2147483648]) {
  try {
    crypto.pbkdf2Sync("password", "salt", iterations, 20, "sha256");
    throw new Error("invalid iterations were accepted");
  } catch (error) {
    if (error.code !== "ERR_OUT_OF_RANGE") throw error;
  }
}

try {
  crypto.pbkdf2Sync("password", "salt", 1, 2147483648, "sha256");
  throw new Error("invalid key length was accepted");
} catch (error) {
  if (error.code !== "ERR_OUT_OF_RANGE") throw error;
}
