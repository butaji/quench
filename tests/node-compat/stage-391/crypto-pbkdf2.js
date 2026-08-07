const crypto = require("crypto");

const expected = "c5e478d59288c841aa530db6845c4c8d962893a0";
const actual = crypto.pbkdf2Sync("password", "salt", 4096, 20, "sha256");
if (actual.toString("hex") !== expected) {
  throw new Error("PBKDF2 vector failed");
}

crypto.pbkdf2("password", "salt", 1, 20, "sha256", (error, value) => {
  if (
    error ||
    value.toString("hex") !== "120fb6cffcf8b32c43e7225256c4f837a86548c9"
  ) {
    throw error || new Error("PBKDF2 callback vector failed");
  }
  console.log("crypto pbkdf2 passed");
});
