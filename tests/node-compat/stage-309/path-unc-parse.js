const assert = require("assert");
const path = require("path").win32;

assert.deepStrictEqual(path.parse("\\\\server\\share\\file_path"), {
  root: "\\\\server\\share\\",
  dir: "\\\\server\\share\\",
  base: "file_path",
  ext: "",
  name: "file_path",
});
