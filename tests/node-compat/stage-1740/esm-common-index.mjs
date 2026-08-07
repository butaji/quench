import * as common from "../../../tests/node/test/common/index.mjs";
if (typeof common.mustCall !== "function") {
  throw new Error("common helper missing");
}
console.log("esm common index passed");
