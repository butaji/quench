const assert = require("assert");
const { scheduler } = require("timers/promises");

assert.throws(() => new scheduler.constructor(), {
  code: "ERR_ILLEGAL_CONSTRUCTOR"
});
assert.throws(() => scheduler.yield.call({}), {
  code: "ERR_INVALID_THIS"
});
assert.throws(() => scheduler.wait.call({}, 1), {
  code: "ERR_INVALID_THIS"
});

(async () => {
  const signal = AbortSignal.abort();
  await assert.rejects(scheduler.wait(1, { signal }), {
    code: "ABORT_ERR"
  });
  console.log("timers promises scheduler validation passed");
})();
