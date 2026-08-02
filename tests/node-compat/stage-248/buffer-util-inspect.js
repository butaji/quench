const { Buffer } = require("buffer");
const util = require("util");

if (util.inspect(Buffer.from([1, 2])) !== "<Buffer 01 02>") {
  throw new Error("util Buffer inspection failed");
}
