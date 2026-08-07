const assert = require("assert");
const { EventEmitter } = require("events");
const { finished } = require("stream");

const streamLike = new EventEmitter();
streamLike.readableEnded = true;
streamLike.readable = true;
assert.throws(() => finished(streamLike, () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});
