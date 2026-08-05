const assert = require("assert");
const http = require("http");

const message = Object.create(http.IncomingMessage.prototype);
let calls = 0;
message
  .once("end", () => calls++)
  .emit("end")
  .emit("end");

assert.strictEqual(calls, 1);
