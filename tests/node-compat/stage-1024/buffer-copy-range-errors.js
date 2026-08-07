const assert = require("assert");
const { Buffer } = require("buffer");
const source = Buffer.alloc(5);
const target = Buffer.alloc(5);

for (
  const [args, name, rule, value] of [
    [[target, -1, 0], "targetStart", ">= 0", -1],
    [[target, 0, -1], "sourceStart", ">= 0 && <= 5", -1],
    [[target, 0, 100], "sourceStart", ">= 0 && <= 5", 100],
    [[target, 0, 0, -1], "sourceEnd", ">= 0", -1],
  ]
) {
  assert.throws(() => source.copy(...args), {
    code: "ERR_OUT_OF_RANGE",
    message:
      `The value of "${name}" is out of range. It must be ${rule}. Received ${value}`,
  });
}
