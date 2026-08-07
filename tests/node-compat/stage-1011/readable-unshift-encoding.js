const { Readable } = require("stream");
let received;
const readable = new Readable({ read() {} });
readable.on("data", (chunk) => {
  received = chunk;
});
readable.unshift("abc", "utf8");
if (!ArrayBuffer.isView(received) || received.toString("utf8") !== "abc") {
  throw new Error("unshift did not apply the requested encoding");
}
