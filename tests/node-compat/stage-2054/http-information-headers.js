const assert = require("assert");
const http = require("http");

const server = http.createServer((req, res) => {
  res.writeInformation(102, { Foo: "Bar" });
  res.end("ok");
});

server.listen(0, () => {
  const req = http.request({ port: server.address().port });
  let sawInformation = false;
  req.on("information", (info) => {
    sawInformation = true;
    assert.strictEqual(info.statusCode, 102);
    assert.strictEqual(info.statusMessage, "Processing");
    assert.strictEqual(info.headers.foo, "Bar");
    assert.deepStrictEqual(info.rawHeaders, ["Foo", "Bar"]);
  });
  req.on("response", (res) => {
    res.on("end", () => {
      assert.strictEqual(sawInformation, true);
      server.close();
    });
    res.resume();
  });
  req.end();
});
