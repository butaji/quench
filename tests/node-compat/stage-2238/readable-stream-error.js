const assert = require("assert");

let controller;
const stream = new ReadableStream({
  start(value) {
    controller = value;
  }
});

const error = new Error("boom");
controller.error(error);
assert.rejects(stream.getReader().read(), (received) => received === error);
console.log("readable stream error passed");
