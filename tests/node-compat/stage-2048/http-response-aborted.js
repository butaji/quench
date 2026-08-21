const assert = require("assert");
const http = require("http");

let serverResponse;
const server = http.createServer((request, response) => {
  serverResponse = response;
  response.writeHead(200);
  response.write("partial");
});

server.listen(0, () => {
  http.get({ port: server.address().port }, (response) => {
    const events = [];
    response.on("aborted", () => events.push("aborted"));
    response.on("error", (error) => {
      assert.strictEqual(error.code, "ECONNRESET");
      events.push("error");
    });
    response.on("close", () => {
      assert.deepStrictEqual(events, ["aborted", "error"]);
      server.close();
      console.log("http response aborted passed");
    });
    serverResponse.destroy();
    response.resume();
  });
});
