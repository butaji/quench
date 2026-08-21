const assert = require("assert");
const { subtle } = globalThis.crypto;
console.log(
  JSON.stringify({
    subtle: typeof subtle,
    importKey: typeof subtle?.importKey,
    decrypt: typeof subtle?.decrypt,
  }),
);
assert.strictEqual(typeof subtle?.importKey, "function");
assert.strictEqual(typeof subtle?.decrypt, "function");
