const vm = require("vm");

const source = 'return "same";';
const cached = vm.compileFunction(source, [], {
  produceCachedData: true,
}).cachedData;
if (
  vm.compileFunction(source, [], { cachedData: cached }).cachedDataRejected !==
    false
) {
  throw new Error("matching cached data was rejected");
}
if (
  vm.compileFunction('return "different";', [], { cachedData: cached })
    .cachedDataRejected !== true
) {
  throw new Error("mismatched cached data was accepted");
}
