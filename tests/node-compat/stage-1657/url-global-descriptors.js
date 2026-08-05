const assert = require("node:assert");
for (const name of ["URL", "URLSearchParams"]) {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
  assert.strictEqual(descriptor.enumerable, false);
  assert.strictEqual(descriptor.writable, true);
  assert.strictEqual(descriptor.configurable, true);
}
console.log("URL global descriptors passed");
