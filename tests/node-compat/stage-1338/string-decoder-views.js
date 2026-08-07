const assert = require("node:assert");
const common = require("../../../tests/node/test/common");
const { StringDecoder } = require("node:string_decoder");

const input = Buffer.from("String for ArrayBufferView tests\n".repeat(8));
for (const [index, view] of common.getArrayBufferViews(input).entries()) {
  const decoder = new StringDecoder("utf8");
  assert.strictEqual(
    decoder.write(view),
    input.toString("utf8"),
    `view ${index}`,
  );
  assert.strictEqual(decoder.end(), "", `end ${index}`);
}
console.log("string decoder views passed");

const decoder = new StringDecoder("utf8");
assert.strictEqual(decoder.write(Buffer.from("E18B", "hex")), "");
assert.strictEqual(decoder.end(), "�");
console.log("string decoder incomplete utf8 passed");
