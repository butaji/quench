const assert = require("node:assert");
const url = require("node:url");

for (
  const path of [
    "\\\\exa mple\\share\\file.txt",
    "\\\\host@name\\share\\file.txt",
    "\\\\host:name\\share\\file.txt",
  ]
) {
  assert.throws(() => url.pathToFileURL(path, { windows: true }), {
    code: "ERR_INVALID_URL",
  });
}
console.log("url pathToFileURL host validation passed");
