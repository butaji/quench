const assert = require("assert");
const http = require("http");

const order = [];
const server = http.createServer();
server.on("close", () => order.push("event"));

server.listen(0, () => {
  order.push("listening");
  assert.strictEqual(server.listening, true);
  const returned = server.close(function (error) {
    order.push("callback");
    assert.strictEqual(this, server);
    assert.strictEqual(error, undefined);
    assert.strictEqual(server.listening, false);
    assert.deepStrictEqual(order, [
      "listening",
      "after-close",
      "event",
      "callback",
    ]);
  });

  assert.strictEqual(returned, server);
  assert.strictEqual(server.listening, false);
  order.push("after-close");
});

const idle = http.createServer();
idle.close((error) => {
  assert.strictEqual(error.code, "ERR_SERVER_NOT_RUNNING");
  console.log("http server close lifecycle passed");
});
