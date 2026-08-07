const assert = require("assert");
const http = require("http");

const server = http.createServer((_request, response) => response.end("ok"));
server.listen(0, () => {
  const request = http.request(
    { port: server.address().port, timeout: 50 },
    (response) => {
      response.resume();
      response.on("end", () => server.close());
    },
  );
  assert.strictEqual(request.timeout, 50);
  assert.strictEqual(request.setTimeout(100), request);
  assert.strictEqual(request.timeout, 100);
  assert.throws(() => request.setTimeout(null), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
  });
  request.end();
});
