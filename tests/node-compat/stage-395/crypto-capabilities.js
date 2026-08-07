const crypto = require("crypto");

if (!crypto.getHashes().includes("sha256")) {
  throw new Error("sha256 must be advertised by getHashes");
}
if (!Array.isArray(crypto.getCiphers())) {
  throw new Error("getCiphers must return an array");
}
if (!crypto.getHashes().includes("sha1")) {
  throw new Error("sha1 must be advertised by getHashes");
}

console.log("crypto capabilities passed");
