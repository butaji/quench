const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => response.end("ok"));
server.listen(0, () => {
  http.get({ port: server.address().port }, (response) => {
    assert.strictEqual(response.readable, true);
    response.on("end", () => {
      assert.strictEqual(response.readable, false);
      server.close();
    });
    response.resume();
  });
});
