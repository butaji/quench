const assert = require("assert");
const http = require("http");

let called = false;
const server = http.createServer(() => {
  called = true;
});
server.listen(0, () => {
  http.get({ port: server.address().port, headers: [] }, (response) => {
    assert.strictEqual(response.statusCode, 400);
    assert.strictEqual(response.headers.connection, "close");
    response.resume().on("end", () => {
      assert.strictEqual(called, false);
      server.close();
    });
  });
});
