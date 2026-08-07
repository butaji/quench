const assert = require("node:assert");
const crypto = require("node:crypto");

Object.defineProperty(Object.prototype, "library", {
  configurable: true,
  set: () => {
    throw new Error("bye, bye, library");
  },
});
try {
  assert.throws(() => crypto.createSign("sha1").sign("PRIVATE KEY"), {
    message: "bye, bye, library",
  });
} finally {
  delete Object.prototype.library;
}
console.log("crypto sign error metadata passed");
