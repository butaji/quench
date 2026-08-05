const assert = require("node:assert");
const crypto = require("node:crypto");

const key = "123456789012345678901234";
const iv = "12345678";
const cipher = crypto.Cipheriv("des-ede3-cbc", key, iv);
const decipher = crypto.Decipheriv("des-ede3-cbc", key, iv);
assert(cipher instanceof crypto.Cipheriv);
assert(decipher instanceof crypto.Decipheriv);
console.log("crypto cipher constructors passed");
