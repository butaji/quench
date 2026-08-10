const { Transform } = require("node:stream");

const upper = new Transform({
  transform(chunk, _encoding, callback) {
    callback(null, chunk.toString().toUpperCase());
  },
});
let output = "";
upper.on("data", (chunk) => output += chunk);
upper.on("end", () => console.log(output));
upper.end("quench streams\n");
