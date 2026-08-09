const assert = require("assert");
const http = require("http");
const { destroy } = require("stream");

const server = http.createServer((request, response) => {
  let closes = 0;
  request.on("close", () => {
    closes++;
    response.end("ok");
    queueMicrotask(() => assert.strictEqual(closes, 1));
  });
  request.resume().on("end", () => destroy(request));
});
server.listen(0, () => {
  const request = http.request({ port: server.address().port }, (response) => {
    response.resume();
    response.on("end", () => server.close());
  });
  request.end("data");
});
