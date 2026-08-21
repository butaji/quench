const assert = require("assert");
const http = require("http");
const { Writable } = require("stream");

const server = http.createServer((req, res) => {
  res.writeHead(200, {
    "Content-Type": "text/plain",
    Connection: "close",
    "Content-Length": 50,
  });
  res.write("aaaaaaaaaabbbbbbbbbbccccccccccdddddddddd");
  process.nextTick(() => res.socket.destroy());
});

server.listen(0, () => {
  const req = http.get({ port: server.address().port });
  req.on("response", (res) => {
    assert.strictEqual(typeof res.socket._handle.close, "function");
    let aborted = false;
    const writable = new Writable({
      write(chunk, encoding, callback) {
        callback();
      },
    });
    res.on("aborted", () => {
      aborted = true;
      writable.end();
    });
    res.on("error", (error) => assert.strictEqual(error.code, "ECONNRESET"));
    writable.on("finish", () => {
      assert.strictEqual(aborted, true);
      server.close();
    });
    res.pipe(writable);
  });
});
