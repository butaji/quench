const vm = require("vm");

const script = new vm.Script("value += 1;");
const sandbox = { value: 1 };

if (script.runInNewContext(sandbox) !== 2) {
  throw new Error("script returned an unexpected value");
}
if (sandbox.value !== 2) throw new Error("sandbox update was not preserved");
if (script.runInNewContext(sandbox) !== 3) {
  throw new Error("script returned an unexpected value");
}
if (sandbox.value !== 3) throw new Error("sandbox reuse was not preserved");
