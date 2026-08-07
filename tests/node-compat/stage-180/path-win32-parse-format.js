const path = require("path").win32;

const parsed = path.parse("C:\\foo\\bar.txt");
if (
  parsed.root !== "C:\\" ||
  parsed.dir !== "C:\\foo" ||
  parsed.base !== "bar.txt" ||
  parsed.ext !== ".txt"
) {
  throw new Error("win32 parse mismatch");
}
if (
  path.format({ dir: "some\\dir", name: "index", ext: "html" }) !==
    "some\\dir\\index.html"
) {
  throw new Error("win32 format mismatch");
}
if (path.basename("C:\\foo\\") !== "foo") {
  throw new Error("win32 basename mismatch");
}
