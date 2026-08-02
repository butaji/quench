const assert = require("assert");
const path = require("path").posix;

assert.deepStrictEqual(path.parse("./"), {
  root: "",
  dir: ".",
  base: ".",
  ext: "",
  name: ".",
});
assert.deepStrictEqual(path.parse("/foo///"), {
  root: "/",
  dir: "/",
  base: "foo",
  ext: "",
  name: "foo",
});
