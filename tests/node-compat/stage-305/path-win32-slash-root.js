const assert = require("assert");
const path = require("path").win32;

assert.deepStrictEqual(path.parse("/foo/bar"), {
  root: "/",
  dir: "/foo",
  base: "bar",
  ext: "",
  name: "bar",
});
