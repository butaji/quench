const assert = require("node:assert");
const { URL } = require("node:url");

const name = Symbol.for("nodejs.util.inspect.custom");
const descriptor = Object.getOwnPropertyDescriptor(URL.prototype, name);
assert.strictEqual(descriptor.enumerable, false);
assert.strictEqual(descriptor.value.name, "[nodejs.util.inspect.custom]");
console.log("URL inspect method passed");
