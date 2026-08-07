const assert = require("assert");

const source = new Uint8Array([1, 2, 3]).buffer;
const transferred = source.transfer();
assert.deepStrictEqual([...new Uint8Array(transferred)], [1, 2, 3]);
assert.strictEqual(transferred.byteLength, 3);
assert.throws(() => ArrayBuffer.prototype.transfer.call({}), TypeError);
console.log("arraybuffer transfer fallback passed");
