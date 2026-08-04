const vm = require("vm");
const { promisify } = require("util");
const fn = vm.runInNewContext("(function () {})");
if (Object.getPrototypeOf(promisify(fn)) === Function.prototype) {
  throw new Error("new-context function prototype was collapsed");
}
