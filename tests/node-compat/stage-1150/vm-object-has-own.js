const vm = require("vm");

const outer = { inherited: true };
const sandbox = Object.create(Object.create(outer));
const context = vm.createContext(sandbox);
const result = vm.runInContext(
  "Object.defineProperty(Object.prototype, 'inner', {value: true, configurable: true}); [typeof Object.hasOwn, Object.hasOwn(this, 'inherited'), Object.hasOwn(this, 'inner'), 'inner' in this, Object.getOwnPropertyDescriptor(this, 'inner')]",
  context,
);
if (
  result[0] !== "function" ||
  result[1] !== false ||
  result[2] !== false ||
  result[3] !== true ||
  result[4] !== undefined
) {
  throw new Error(`unexpected Object.hasOwn result: ${result}`);
}
