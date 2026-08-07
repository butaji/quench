const { Transform, compose } = require("stream");

let output = "";
const upper = new Transform({
  transform(chunk, encoding, callback) {
    callback(null, chunk.toString().toUpperCase());
  },
});
const duplicate = new Transform({
  transform(chunk, encoding, callback) {
    callback(null, chunk.toString() + chunk.toString());
  },
});
const composed = compose(duplicate, upper);
composed.on("data", (chunk) => (output += chunk.toString()));
composed.end("a");

queueMicrotask(() => {
  if (output !== "AA") throw new Error(`unexpected compose output: ${output}`);
});
