const vm = require("vm");

const source = "function x() {} const y = x();";
const script = new vm.Script(source);
const cachedData = script.createCachedData();
if (!Buffer.isBuffer(cachedData)) {
  throw new Error("cached data was not a Buffer");
}
if (new vm.Script(source, { cachedData }).cachedDataRejected) {
  throw new Error("matching script cache was rejected");
}
if (!new vm.Script(source, { produceCachedData: true }).cachedDataProduced) {
  throw new Error("script cache was not produced");
}
