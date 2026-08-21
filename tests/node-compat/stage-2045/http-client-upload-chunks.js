const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  request.setEncoding("utf8");
  const chunks = [];
  request.on("data", (chunk) => chunks.push(chunk));
  request.on("end", () => {
    assert.deepStrictEqual(chunks, ["1\n", "2\n", "3\n"]);
    response.end("hello\n");
  });
});

server.listen(0, () => {
  const request = http.request(
    { port: server.address().port, method: "POST", path: "/" },
    (response) => {
      response.setEncoding("utf8");
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => {
        assert.deepStrictEqual(chunks, ["hello\n"]);
        server.close();
        console.log("http upload chunks passed");
      });
    },
  );
  request.write("1\n");
  request.write("2\n");
  request.write("3\n");
  request.end();
});
