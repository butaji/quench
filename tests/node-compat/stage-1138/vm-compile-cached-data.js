const vm = require("vm");

const compiled = vm.compileFunction('return "cached";', [], {
  produceCachedData: true,
});
if (!compiled.cachedDataProduced || compiled.cachedData.length === 0) {
  throw new Error("cached data metadata was not produced");
}
if (
  vm.compileFunction('return "cached";', [], {
    cachedData: compiled.cachedData,
  })() !== "cached"
) {
  throw new Error("cached data could not be consumed");
}
