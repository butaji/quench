const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  request.resume();
  request.on("end", () => response.end("ok"));
});

server.listen(0, "127.0.0.1", () => {
  let responses = 0;
  const makeRequest = (options) => {
    Object.defineProperty(Object.prototype, "hostname", {
      configurable: true,
      get() {
        throw new Error("inherited hostname was accessed");
      }
    });
    const request = http.request(options, (response) => {
      responses += 1;
      assert.strictEqual(response.statusCode, 200);
      response.resume();
      response.on("end", () => {
        if (responses === 2) server.close();
      });
    });
    request.on("error", (error) => assert.fail(error));
    request.end();
    delete Object.prototype.hostname;
  };
  makeRequest({ host: "127.0.0.1", port: server.address().port, path: "/" });
  makeRequest({
    __proto__: null,
    host: "127.0.0.1",
    port: server.address().port,
    path: "/"
  });
});
