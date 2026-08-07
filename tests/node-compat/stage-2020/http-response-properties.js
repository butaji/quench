const assert = require("assert");
const http = require("http");
const server = http.createServer((req, res) => {
  console.log(JSON.stringify({
    reqSocketHwm: req.socket?.writableHighWaterMark,
    resHwm: res.writableHighWaterMark,
    length: res.writableLength,
    writable: res.writable,
    objectMode: res.writableObjectMode,
  }));
  assert.strictEqual(
    res.writableHighWaterMark,
    req.socket.writableHighWaterMark,
  );
  assert.strictEqual(res.writableLength, 0);
  res.write("");
  const length = res.writableLength;
  res.write("asd");
  assert.strictEqual(res.writableLength, length + 8);
  res.end();
  res.once("finish", () => server.close());
});
server.listen(
  0,
  () => http.get({ port: server.address().port }, (res) => res.resume()),
);
