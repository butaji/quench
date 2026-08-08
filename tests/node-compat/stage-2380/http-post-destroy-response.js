const assert = require("assert");
const http = require("http");
const { destroy } = require("stream");

const server = http.createServer((request, response) => {
  request.resume().on("end", () => destroy(request));
  request.once("close", () => response.end("hello"));
});

server.listen(0, () => {
  const request = http.request({ method: "POST", port: server.address().port });
  request.once("response", (response) => {
    const chunks = [];
    response.on("data", (chunk) => chunks.push(chunk));
    response.once("end", () => {
      assert.strictEqual(Buffer.concat(chunks).toString(), "hello");
      server.close();
      console.log("http post-destroy response passed");
    });
  });
  request.end("asd");
});
