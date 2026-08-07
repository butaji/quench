const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.readFileSync(new URL("http://example.org")), {
  code: "ERR_INVALID_URL_SCHEME",
  name: "TypeError",
});
assert.throws(() => fs.readFile(new URL("http://example.org"), () => {}), {
  code: "ERR_INVALID_URL_SCHEME",
  name: "TypeError",
});
console.log("Filesystem URL scheme validation passed");
