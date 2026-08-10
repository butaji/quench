const assert = require("assert");
const path = require("path");
const {
  isUnderMountPoint,
  getRelativePath,
  isAbsolutePath,
} = require("internal/vfs/router");

const mount = path.resolve("/app");
assert.strictEqual(isUnderMountPoint(path.join(mount, "src"), mount), true);
assert.strictEqual(isUnderMountPoint(path.resolve("/app2"), mount), false);
assert.strictEqual(
  getRelativePath(path.join(mount, "src/index.js"), mount),
  "/src/index.js",
);
assert.strictEqual(isAbsolutePath("/app"), true);
