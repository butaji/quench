const assert = require("assert");
const http = require("http");

const server = http.createServer(() => {
  throw new Error("server should not receive the request");
});

server.listen(0, () => {
  const request = http.get({ port: server.address().port });
  let errored = false;
  request.on("error", (error) => {
    errored = true;
    assert.strictEqual(error.message, "socket hang up");
    assert.strictEqual(error.code, "ECONNRESET");
  });
  request.on("close", () => {
    assert.strictEqual(errored, true);
    assert.strictEqual(request.destroyed, true);
    server.close();
  });
  request.destroy();
});
