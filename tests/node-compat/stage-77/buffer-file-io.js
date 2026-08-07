const fs = require("fs");
const { Buffer } = require("buffer");

const path = "/tmp/quench-node-stage-77.bin";
const input = Buffer.from([0, 1, 127, 128, 255]);
fs.writeFileSync(path, input);
const output = fs.readFileSync(path);
if (!Buffer.isBuffer(output) || output.length !== input.length) {
  throw new Error("binary length mismatch");
}
for (let i = 0; i < input.length; i++) {
  if (output[i] !== input[i]) throw new Error("binary content mismatch");
}
