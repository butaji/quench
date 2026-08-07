const vm = require("vm");

const sandbox = {};
vm.runInNewContext("this.Proxy = Proxy", sandbox);
if (typeof sandbox.Proxy !== "function") {
  throw new Error("context Proxy constructor was not copied");
}
