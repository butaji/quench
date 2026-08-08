const assert = require("assert");
const http = require("http");
const { listenerCount } = require("events");

const server = http.createServer(() => {
  throw new Error("server should not receive the request");
});
const controller = new AbortController();

server.listen(0, () => {
  const request = http.get({
    port: server.address().port,
    signal: controller.signal
  });
  assert.strictEqual(listenerCount(controller.signal, "abort"), 1);
  assert.strictEqual(request.aborted, false);
  request.on("error", (error) => {
    assert.strictEqual(error.name, "AbortError");
    assert.strictEqual(error.code, "ABORT_ERR");
    assert.strictEqual(request.aborted, false);
    assert.strictEqual(request.destroyed, true);
    server.close();
  });
  controller.abort();
});
