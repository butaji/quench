const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  assert.strictEqual(request.method, "POST");
  request.setEncoding("utf8");
  let body = "";
  request.on("data", (chunk) => (body += chunk));
  request.on("end", () => {
    assert.strictEqual(body, "payload");
    response.end("ok");
  });
});

server.listen(0, () => {
  const request = http.request(
    { port: server.address().port, method: "POST" },
    (response) => {
      response.resume().on("end", () => server.close());
    },
  );
  assert.strictEqual(request.end("payload"), request);
});
