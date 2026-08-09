const assert = require("assert");
const { finished } = require("stream");

const emitterLike = {
  once() {},
  emit() {}
};

assert.throws(() => finished(emitterLike, () => {}), {
  code: "ERR_INVALID_ARG_TYPE"
});
