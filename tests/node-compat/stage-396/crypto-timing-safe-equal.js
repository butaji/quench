const crypto = require("crypto");

if (!crypto.timingSafeEqual(Buffer.from("same"), Buffer.from("same"))) {
  throw new Error("equal buffers must compare true");
}
if (crypto.timingSafeEqual(Buffer.from("same"), Buffer.from("diff"))) {
  throw new Error("different buffers must compare false");
}

let error;
try {
  crypto.timingSafeEqual(Buffer.from("short"), Buffer.from("longer"));
} catch (caught) {
  error = caught;
}
if (!error || error.code !== "ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH") {
  throw new Error("unequal buffers must report their length error");
}

console.log("crypto timing safe equal passed");
