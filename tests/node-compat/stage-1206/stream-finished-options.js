const assert = require("assert");
const { Readable } = require("stream");
const { finished } = require("stream/promises");

assert.throws(() => finished(new Readable(), { cleanup: 2 }), {
  code: "ERR_INVALID_ARG_TYPE",
});
