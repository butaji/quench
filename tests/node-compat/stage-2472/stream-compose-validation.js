const assert = require("assert");
const { compose, PassThrough, Readable, Writable } = require("stream");

assert.throws(() => compose(), { code: "ERR_MISSING_ARGS" });
assert.throws(() => compose(new Writable(), new PassThrough()), {
  code: "ERR_INVALID_ARG_VALUE",
});
assert.throws(
  () =>
    compose(new PassThrough(), new Readable({ read() {} }), new PassThrough()),
  { code: "ERR_INVALID_ARG_VALUE" },
);

assert.doesNotThrow(() =>
  compose(Readable.from(["value"]), async function* (source) {
    yield* source;
  })
);
assert.doesNotThrow(() =>
  compose(
    async function* (source) {
      yield* source;
    },
    new Writable({
      write(_chunk, _encoding, callback) {
        callback();
      },
    }),
  )
);
