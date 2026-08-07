const assert = require("assert");
const { Readable, finished } = require("stream");

const stream = new Readable({ read() {} });
assert.throws(() => finished(stream, "foo"), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => finished(stream, "foo", () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => finished(stream, {}, "foo"), {
  code: "ERR_INVALID_ARG_TYPE",
});
