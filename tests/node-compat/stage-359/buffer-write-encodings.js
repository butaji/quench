const { Buffer } = require("buffer");
const value = Buffer.alloc(9);
if (
  value.write("foo", 0, "utf8") !== 3 ||
  value.toString("utf8", 0, 3) !== "foo"
) {
  throw new Error("utf8 write");
}
value.fill(0);
if (value.write("foo", 0, "ucs2") !== 6 || value[1] !== 0) {
  throw new Error("ucs2 write");
}
