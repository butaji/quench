const { Buffer } = require("buffer");

const high = "\ud800";
const low = "\udc00";
const pair = "\ud83d\ude00";

const cases = [
  [high, [0xef, 0xbf, 0xbd]],
  [low, [0xef, 0xbf, 0xbd]],
  [pair, [0xf0, 0x9f, 0x98, 0x80]],
  [`a${high}b`, [0x61, 0xef, 0xbf, 0xbd, 0x62]],
];

for (const [input, expected] of cases) {
  const output = Buffer.from(input, "utf8");
  if (output.length !== expected.length) {
    throw new Error(`length mismatch for ${JSON.stringify(input)}`);
  }
  for (let index = 0; index < expected.length; index++) {
    if (output[index] !== expected[index]) {
      throw new Error(`byte mismatch at ${index} for ${JSON.stringify(input)}`);
    }
  }
}

const value = `a${high}b`.repeat(100);
const actual = new Uint8Array(Buffer.from(value, "utf8"));
const reference = new TextEncoder().encode(value);
if (actual.length !== reference.length) throw new Error("long length mismatch");
for (let index = 0; index < actual.length; index++) {
  if (actual[index] !== reference[index]) throw new Error("long byte mismatch");
}
