const assert = require("assert");

const { stringToFlags } = require("internal/fs/utils");

const expected = {
  r: 0,
  "r+": 2,
  rs: 1052674,
  "rs+": 1052674,
  w: 577,
  wx: 705,
  "w+": 578,
  "wx+": 706,
  a: 1089,
  ax: 1217,
  "a+": 1090,
  "ax+": 1218,
  as: 1053761,
  "as+": 1053762,
};

for (const [flags, value] of Object.entries(expected)) {
  assert.strictEqual(stringToFlags(flags), value, flags);
}

assert.throws(() => stringToFlags("invalid"), {
  code: "ERR_INVALID_ARG_VALUE",
});
console.log("stringToFlags passed");
