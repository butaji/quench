const common = require(process.cwd() + "/tests/node/test/common/index.js");
if (typeof common.mustCall !== "function") {
  throw new Error("common CJS entry missing mustCall");
}
console.log("common CJS entry passed");
