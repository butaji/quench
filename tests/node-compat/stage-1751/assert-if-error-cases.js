const assert = require("assert");

const cases = [
  ["type", () => assert.ifError(new TypeError()), {
    message: "ifError got unwanted exception: TypeError",
  }],
  ["stack", () => assert.ifError({ stack: false }), {
    message: "ifError got unwanted exception: { stack: false }",
  }],
  ["empty", () => assert.ifError({ constructor: null, message: "" }), {
    message: "ifError got unwanted exception: ",
  }],
  ["false", () => assert.ifError(false), {
    message: "ifError got unwanted exception: false",
  }],
];
for (const [name, fn, expected] of cases) {
  try {
    assert.throws(fn, expected);
  } catch (error) {
    console.log(
      name,
      JSON.stringify({ actual: error.message, expected: expected.message }),
    );
    throw error;
  }
  console.log(`${name}:ok`);
}
