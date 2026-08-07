const common = require("../common");

if (common.hasCrypto !== true) throw new Error("crypto capability was missing");
if (typeof common.skip !== "function") {
  throw new Error("skip helper was missing");
}
if (typeof common.skipIfInspectorDisabled !== "function") {
  throw new Error("inspector skip helper was missing");
}
