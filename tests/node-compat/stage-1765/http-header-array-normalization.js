const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  assert.strictEqual(request.headers["x-num"], "1");
  response.setHeader("content-type", ["A", "B"]);
  response.setHeader("x-custom", ["A", "B"]);
  response.end();
});
server.listen(0, () => {
  http.get(
    { port: server.address().port, headers: { "x-num": 1 } },
    (response) => {
      assert.strictEqual(response.headers["content-type"], "A");
      assert.strictEqual(response.headers["x-custom"], "A, B");
      response.resume().on("end", () => server.close());
    },
  );
});
