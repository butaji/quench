const vm = require("vm");
const sandbox = {};
Object.defineProperty(sandbox, "prop", { get: () => "foo" });
const expected = Object.getOwnPropertyDescriptor(sandbox, "prop");
const actual = vm.runInContext(
  'Object.getOwnPropertyDescriptor(this, "prop")',
  vm.createContext(sandbox),
);
for (const key of Object.keys(expected)) {
  if (actual[key] !== expected[key]) {
    throw new Error(`descriptor mismatch: ${key}`);
  }
}
