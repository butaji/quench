const assert = require("assert");
const http = require("http");
const { finished } = require("stream");

const server = http.createServer((_request, response) => {
  response.write("payload");
});

server.listen(0, () => {
  http
    .request({ port: server.address().port })
    .on("response", (response) => {
      response.once("readable", () => response.destroy());
      finished(response, (error) => {
        assert.ok(error === undefined || error.code === "ECONNRESET");
        server.close();
      });
    })
    .end();
});
