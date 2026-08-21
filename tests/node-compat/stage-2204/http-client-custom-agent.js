const assert = require("assert");
const http = require("http");
const { Duplex } = require("stream");

class Agent extends http.Agent {
  createConnection() {
    const socket = new Duplex({
      read() {
        this.push(
          "HTTP/1.1 200 OK\\r\\nTransfer-Encoding: chunked\\r\\n\\r\\n",
        );
        this.push("b\\r\\nhello world\\r\\n0\\r\\n\\r\\n");
        this.push(null);
      },
      write(_chunk, _encoding, callback) {
        callback();
      },
    });
    let once = false;
    socket._read = function () {
      if (once) return this.push(null);
      once = true;
      this.push(
        "HTTP/1.1 200 OK\\\\r\\\\nTransfer-Encoding: chunked\\\\r\\\\n\\\\r\\\\n",
      );
      this.push("b\\\\r\\\\nhello world\\\\r\\\\n0\\\\r\\\\n\\\\r\\\\n");
    };
    socket._write = function (_data, _encoding, callback) {
      callback();
    };
    socket.destroy = socket.destroySoon = function () {
      this.writable = false;
    };
    return socket;
  }
}

let body = "";
const request = http.request({ agent: new Agent() }, (response) => {
  response.on("data", (chunk) => {
    body += chunk;
  });
  response.on("end", () => {
    assert.strictEqual(body, "hello world");
  });
});
request.end();
