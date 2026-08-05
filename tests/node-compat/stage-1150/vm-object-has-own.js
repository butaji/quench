const vm = require("vm");

const outer = { inherited: true };
const sandbox = Object.create(Object.create(outer));
const context = vm.createContext(sandbox);
const result = vm.runInContext(
  "[typeof Object.hasOwn, Object.hasOwn(this, 'inherited')]",
  context
);
if (result[0] !== "function" || result[1] !== false)
  throw new Error(`unexpected Object.hasOwn result: ${result}`);
