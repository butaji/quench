const vm = require("vm");

const sandbox = {};
const result = vm.runInNewContext(
  'typeof process + ":" + typeof Object',
  sandbox,
);
if (result !== "undefined:function") {
  throw new Error("new context exposed an unexpected host global");
}
