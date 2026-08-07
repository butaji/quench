const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  response.setHeader("X-Test", "value");
  assert.deepStrictEqual(response.getHeaders(), { "x-test": "value" });
  assert.deepStrictEqual(response.getHeaderNames(), ["x-test"]);
  assert.strictEqual(response.getHeader("X-Test"), "value");
  assert.strictEqual(response.flushHeaders(), response);
  assert.strictEqual(
    response.writeEarlyHints({ link: "</style.css>; rel=preload" }),
    response
  );
  response.end("ok");
});

server.listen(0, () => {
  const request = http.get(
    `http://localhost:${server.address().port}`,
    (response) => {
      let body = "";
      response.setEncoding("utf8");
      response.on("data", (chunk) => (body += chunk));
      response.on("end", () => {
        assert.strictEqual(body, "ok");
        server.close();
      });
    }
  );
  assert.strictEqual(request.flushHeaders(), request);
});

console.log("http response header surface passed");
