const assert = require("assert");
const http = require("http");
let clientError = false;

const server = http.createServer((request, response) => {
  assert.strictEqual(response.closed, false);
  request.pipe(response);
  response.on(
    "error",
    () => assert.fail("server response should not emit error"),
  );
  response.on("close", () => {
    assert.strictEqual(response.closed, true);
    assert.strictEqual(clientError, true);
    server.close();
  });
  const error = new Error("destroy");
  response.destroy(error);
  assert.strictEqual(response.errored, error);
});

server.listen(0, () => {
  http
    .request({ port: server.address().port, method: "PUT" })
    .on("error", () => {
      clientError = true;
    })
    .write("input");
});
