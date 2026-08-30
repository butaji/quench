const assert = require("assert");
const https = require("https");
const vm = require("vm");

assert.strictEqual(typeof https.request, "function");
assert.strictEqual(typeof https.get, "function");
assert.notStrictEqual(https.request, vm.runInNewContext);
assert.notStrictEqual(https.get, vm.createContext);
