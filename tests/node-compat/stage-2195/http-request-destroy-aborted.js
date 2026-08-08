const assert = require("assert");
const http = require("http");

const server = http.createServer(() => {
  throw new Error("server should not receive the request");
});

server.listen(0, () => {
  const request = http.get({ port: server.address().port });
  request.on("error", (error) => {
    assert.strictEqual(error.code, "ECONNRESET");
    assert.strictEqual(request.aborted, false);
    server.close();
  });
  assert.strictEqual(request.aborted, false);
  request.destroy();
  assert.strictEqual(request.aborted, false);
  assert.strictEqual(request.destroyed, true);
});
