const vm = require("vm");

const first = Symbol("first");
const second = Symbol("second");
const context = vm.createContext({ [first]: true, [second]: true });
const keys = vm.runInContext("Reflect.ownKeys(this)", context);
if (!keys.includes(first) || !keys.includes(second)) {
  throw new Error("symbol context keys were not forwarded");
}
