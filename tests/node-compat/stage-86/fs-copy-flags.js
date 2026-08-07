const fs = require("fs");
const { internalBinding } = require("internal/test/binding");

if (typeof fs.constants.COPYFILE_EXCL !== "number") {
  throw new Error("copy flag missing");
}
if (typeof internalBinding("uv").UV_ENOENT !== "number") {
  throw new Error("uv constant missing");
}
