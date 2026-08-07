const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => response.end("ok"));
server.listen(0, () => {
  http.get({ port: server.address().port }, (response) => {
    assert.strictEqual(response.destroyed, false);
    const events = [];
    response.on("end", () => events.push("end"));
    response.on("close", () => {
      events.push("close");
      assert.deepStrictEqual(events, ["end", "close"]);
      assert.strictEqual(response.destroyed, true);
      server.close();
    });
    response.resume();
  });
});
