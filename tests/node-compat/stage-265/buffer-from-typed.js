const { Buffer } = require("buffer");

const input = new Uint32Array(4).fill(42);
const output = Buffer.from(input);
if (output.length !== 4) throw new Error(`length ${output.length}`);
for (let index = 0; index < 4; index++) {
  if (output[index] !== 42) throw new Error(`value ${index}: ${output[index]}`);
}
