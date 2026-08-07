const { Transform } = require("stream");
const transform = new Transform({
  highWaterMark: 1,
  transform: (_chunk, _encoding, callback) => queueMicrotask(callback),
});
if (transform._writableState.needDrain) {
  throw new Error("needDrain started true");
}
transform.write("large");
if (!transform._writableState.needDrain) {
  throw new Error("needDrain was not set");
}
