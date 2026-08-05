const assert = require("node:assert");
const { URL } = require("node:url");
for (const name of ["createObjectURL", "revokeObjectURL"]) {
  const descriptor = Object.getOwnPropertyDescriptor(URL, name);
  assert.strictEqual(descriptor.configurable, true);
  assert.strictEqual(descriptor.enumerable, true);
  assert.strictEqual(descriptor.writable, true);
}
console.log("URL static object URL methods passed");
