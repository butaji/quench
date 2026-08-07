const path = require("path");

if (path.posix !== path || path.win32 === path) {
  throw new Error("path namespace mismatch");
}
if (typeof path.win32 !== "object") throw new Error("win32 namespace missing");
if (path.parse("/foo/bar.txt").name !== "bar") {
  throw new Error("path parse mismatch");
}
if (path.format({ name: "x", ext: "txt" }) !== "x.txt") {
  throw new Error("path format mismatch");
}
for (const value of [null, {}, true, 1]) {
  try {
    path.parse(value);
    throw new Error("accepted invalid path");
  } catch (error) {
    if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
  }
  if (value !== null && typeof value === "object") continue;
  try {
    path.format(value);
    throw new Error("accepted invalid path object");
  } catch (error) {
    if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
  }
}
