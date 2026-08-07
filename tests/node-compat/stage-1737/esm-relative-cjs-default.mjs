import tmpdir from "../../../tests/node/test/common/tmpdir.js";
if (typeof tmpdir?.refresh !== "function") {
  throw new Error("relative CommonJS default import missing");
}
tmpdir.refresh();
console.log("esm relative cjs default passed");
