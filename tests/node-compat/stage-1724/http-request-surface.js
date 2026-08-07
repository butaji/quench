const assert = require("node:assert");
const http = require("node:http");

const server = http.createServer((request, response) => {
  assert.strictEqual(request.method, "POST");
  assert.strictEqual(request.headers["x-test"], "ok");
  response.end("done");
});
server.listen(43212, function () {
  const request = http.request(
    {
      port: this.address().port,
      method: "POST",
      path: "/",
      headers: { "x-test": "ok" },
    },
    (response) => {
      response.on("data", (value) => {
        assert(Buffer.isBuffer(value));
        assert.strictEqual(value.toString(), "done");
      });
      response.on("end", () => {
        server.close();
        console.log("http request surface passed");
      });
    },
  );
  assert.strictEqual(request.removeHeader("date"), request);
  request.end("body");
});
