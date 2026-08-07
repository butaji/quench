const { Buffer } = require("buffer");

const buffer = Buffer.from([1, 2, 3]);
if (JSON.stringify(buffer) !== '{"type":"Buffer","data":[1,2,3]}') {
  throw new Error("Buffer JSON shape failed");
}
const parsed = JSON.parse(JSON.stringify(buffer));
if (!Buffer.from(parsed).equals(buffer)) {
  throw new Error("Buffer JSON round-trip failed");
}
