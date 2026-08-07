const assert = require("node:assert");
const http = require("node:http");

const server = http.createServer(() => {
  throw new Error("aborted request must not dispatch");
});

server.listen(0, () => {
  const request = http.request({
    host: "127.0.0.1",
    port: server.address().port,
  });
  request.once("abort", () => {
    assert.strictEqual(request.aborted, true);
    server.close();
    console.log("http request abort passed");
  });
  request.abort();
  request.end();
});
