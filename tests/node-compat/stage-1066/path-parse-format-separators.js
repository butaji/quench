const assert = require("assert");
const path = require("path");

const input = "C:\\another_path\\DIR\\1\\2\\33\\\\index";
const parsed = path.win32.parse(input);
assert.strictEqual(path.win32.parse("C:").dir, "C:");
assert.strictEqual(path.win32.dirname("C:"), "C:");
assert.strictEqual(path.win32.parse("\\foo\\C:").base, "C:");
assert.deepStrictEqual(path.win32.parse("t"), {
  base: "t",
  name: "t",
  root: "",
  dir: "",
  ext: "",
});
assert.deepStrictEqual(path.win32.parse("/foo/bar"), {
  root: "/",
  dir: "/foo",
  base: "bar",
  ext: "",
  name: "bar",
});
const unc = "\\\\server\\share\\file_path";
assert.strictEqual(path.win32.format(path.win32.parse(unc)), unc);
for (
  const value of [
    "C:\\path\\dir\\index.html",
    "C:\\another_path\\DIR\\1\\2\\33\\\\index",
    "\\",
    "\\foo\\C:",
    "C:",
    "C:.",
    "C:\\",
    "\\\\server\\share\\file_path",
    "",
  ]
) {
  assert.strictEqual(path.win32.format(path.win32.parse(value)), value);
}
assert.deepStrictEqual(path.win32.parse("C:\\"), {
  root: "C:\\",
  dir: "C:\\",
  base: "",
  ext: "",
  name: "",
});
assert.strictEqual(path.win32.dirname("C:\\"), "C:\\");
assert.strictEqual(parsed.dir, "C:\\another_path\\DIR\\1\\2\\33\\\\");
assert.strictEqual(path.win32.format(parsed), input);
