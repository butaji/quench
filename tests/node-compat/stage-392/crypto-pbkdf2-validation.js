const crypto = require("crypto");

for (const iterations of [0, -1]) {
  let error;
  try {
    crypto.pbkdf2("password", "salt", iterations, 8, "sha256", () => {});
  } catch (caught) {
    error = caught;
  }
  if (!error || error.code !== "ERR_OUT_OF_RANGE") {
    throw new Error("invalid iterations must throw synchronously");
  }
}

let error;
try {
  crypto.pbkdf2("password", "salt", 1, 8, undefined, () => {});
} catch (caught) {
  error = caught;
}
if (!error || error.code !== "ERR_INVALID_ARG_TYPE") {
  throw new Error("missing digest must throw synchronously");
}

console.log("crypto pbkdf2 validation passed");
