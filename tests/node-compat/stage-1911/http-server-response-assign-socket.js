const assert = require("assert");
const http = require("http");
const { Writable } = require("stream");

const request = {
  method: "GET",
  httpVersionMajor: 1,
  httpVersionMinor: 1,
};
const response = new http.ServerResponse(request);

assert(response instanceof http.ServerResponse);
assert.strictEqual(response.req, request);
assert.strictEqual(typeof response.assignSocket, "function");

const chunks = [];
const socket = new Writable({
  write(chunk, encoding, callback) {
    chunks.push(Buffer.from(chunk));
    callback();
  },
});
let socketEvent;
response.once("socket", (value) => {
  socketEvent = value;
});

assert.strictEqual(response.assignSocket(socket), undefined);
assert.strictEqual(socketEvent, socket);
assert.strictEqual(socket._httpMessage, response);
assert.strictEqual(response.socket, socket);
assert.throws(() => response.assignSocket(socket), {
  code: "ERR_HTTP_SOCKET_ASSIGNED",
});

response.end("hello world", () => {
  assert.strictEqual(chunks.length, 2);
  assert(chunks[0].toString().endsWith("hello world"));
  assert.strictEqual(chunks[1].length, 0);
  console.log("http ServerResponse assignSocket passed");
});
