const assert = require("node:assert");

const value = createExternalizableString("compat");
assert.strictEqual(value, "compat");
assert.strictEqual(isOneByteString(value), true);
assert.strictEqual(externalizeString(value), undefined);
console.log("externalizable string globals passed");
