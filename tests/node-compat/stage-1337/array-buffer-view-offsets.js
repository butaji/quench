const assert = require("node:assert");
const common = require("../../../tests/node/test/common");

const input = Buffer.from("String for ArrayBufferView tests\n".repeat(8));
for (const view of common.getArrayBufferViews(input)) {
  assert.strictEqual(view.byteLength, input.byteLength);
  assert.deepStrictEqual(
    Array.from(new Uint8Array(view.buffer, view.byteOffset, view.byteLength)),
    Array.from(input),
  );
}
console.log("array buffer view offsets passed");
