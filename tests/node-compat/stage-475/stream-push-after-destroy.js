const { Readable } = require("stream");

const stream = new Readable({ read() {} });
stream.destroy();
if (stream.push("late") !== false || !stream.destroyed) {
  throw new Error("push-after-destroy result was incorrect");
}

console.log("stream push after destroy passed");
