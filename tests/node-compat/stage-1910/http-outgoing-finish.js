const assert = require("assert");
const http = require("http");

const events = [];
const server = http.createServer((request, response) => {
  assert.strictEqual(response.writable, true);
  assert.strictEqual(response.finished, false);
  assert.strictEqual(response.writableEnded, false);
  assert.strictEqual(response.writableFinished, false);
  request.on("end", () => events.push("server-end"));
  response.end("ok");
  assert.strictEqual(response.writable, true);
  assert.strictEqual(response.finished, true);
  assert.strictEqual(response.writableEnded, true);
});
server.listen(0, () => {
  const request = http.request({ port: server.address().port, path: "/" });
  assert.strictEqual(request.finished, false);
  assert.strictEqual(request.writable, true);
  assert.strictEqual(request.writableFinished, false);
  request.once("finish", () => events.push("client-finish"));
  request.once("response", (response) => {
    response.resume();
    response.once("end", () => {
      assert.strictEqual(request.finished, true);
      assert.strictEqual(request.writableFinished, true);
      assert.strictEqual(events[0], "client-finish");
      server.close();
    });
  });
  request.end();
});
