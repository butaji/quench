const assert = require("assert");
const http = require("http");

const server = http.createServer((req, res) => {
  res.writeProcessing();
  res.writeProcessing();
  res.end("ok");
});
server.listen(0, () => {
  const req = http.request({ port: server.address().port });
  let count = 0;
  req.on("information", (info) => {
    count++;
    assert.strictEqual(info.statusCode, 102);
    assert.strictEqual(info.statusMessage, "Processing");
  });
  req.on("response", (res) => {
    res.on("end", () => {
      assert.strictEqual(count, 2);
      server.close();
    });
    res.resume();
  });
  req.end();
});
