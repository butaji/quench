const vm = require("vm");
const context = {};
let value;
Object.defineProperty(context, "getter", { get: () => "ok" });
Object.defineProperty(context, "setter", {
  get: () => `ok=${value}`,
  set: (next) => {
    value = next;
  },
});
const result = vm.runInContext(
  'setter = "test"; [getter, setter]',
  vm.createContext(context),
);
if (result[0] !== "ok" || result[1] !== "ok=test") {
  throw new Error("VM accessors were not preserved");
}
