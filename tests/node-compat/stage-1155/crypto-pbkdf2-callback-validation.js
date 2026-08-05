const crypto = require("crypto");

try {
  crypto.pbkdf2("password", "salt", 1, 20, "sha1");
  throw new Error("missing callback was accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}
