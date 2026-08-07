const assert = require("node:assert");
const url = require("node:url");

for (const value of [null, undefined, 1, {}, true]) {
  assert.throws(() => url.fileURLToPath(value), {
    code: "ERR_INVALID_ARG_TYPE",
  });
}
assert.throws(() => url.fileURLToPath("https://a/b/c"), {
  code: "ERR_INVALID_URL_SCHEME",
});
assert.throws(() => url.fileURLToPath(new URL("file://host/a")), {
  code: "ERR_INVALID_FILE_URL_HOST",
});
console.log("url fileURLToPath errors passed");
