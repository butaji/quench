const vm = require("vm");

if (new vm.Script("// sourceMappingURL=ignored").sourceMapURL !== undefined) {
  throw new Error("malformed source map comment was accepted");
}
if (
  new vm.Script("//# sourceMappingURL=sourcemap.json").sourceMapURL !==
    "sourcemap.json"
) {
  throw new Error("source map URL was not extracted");
}
