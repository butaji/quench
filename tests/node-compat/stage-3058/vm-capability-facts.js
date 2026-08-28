const assert = require("assert");
const vm = require("vm");

assert.strictEqual(typeof vm.runInNewContext, "function");
assert.strictEqual(typeof vm.createContext, "function");
assert.strictEqual(typeof vm.runInContext, "function");
assert.strictEqual(typeof vm.isContext, "function");
