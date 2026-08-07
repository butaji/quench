const { Readable } = require("stream");
const values = [];
const readable = new Readable({
  defaultEncoding: "hex",
  read() {},
});
readable.on("data", (chunk) => values.push(chunk));
readable.push("ab");
readable.push("xy", "utf8");
if (values.length !== 2 || values[0].length !== 1 || values[0][0] !== 171) {
  throw new Error("default push encoding was not applied");
}
if (values[1].toString("utf8") !== "xy") {
  throw new Error("explicit push encoding was not applied");
}
