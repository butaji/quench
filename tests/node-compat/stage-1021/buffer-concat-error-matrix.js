const assert = require("assert");
const { Buffer } = require("buffer");
const cases = [
  [
    undefined,
    'The "list" argument must be an instance of Array. Received undefined',
  ],
  [null, 'The "list" argument must be an instance of Array. Received null'],
  [
    Buffer.from("h"),
    'The "list" argument must be an instance of Array. Received an instance of Buffer',
  ],
  [
    [42],
    'The "list[0]" argument must be an instance of Buffer or Uint8Array. Received type number (42)',
  ],
  [
    ["hello", Buffer.from("w")],
    "The \"list[0]\" argument must be an instance of Buffer or Uint8Array. Received type string ('hello')",
  ],
  [
    [Buffer.from("h"), 3],
    'The "list[1]" argument must be an instance of Buffer or Uint8Array. Received type number (3)',
  ],
];
for (const [value, message] of cases) {
  assert.throws(() => Buffer.concat(value), {
    code: "ERR_INVALID_ARG_TYPE",
    message,
  });
}
