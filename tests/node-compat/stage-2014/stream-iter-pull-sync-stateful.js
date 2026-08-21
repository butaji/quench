const assert = require("assert");
const { bytesSync, fromSync, pullSync } = require("stream/iter");

const decode = (value) => new TextDecoder().decode(value);
const encoder = new TextEncoder();

const stateful = {
  transform(source) {
    return (function* () {
      for (const chunks of source) {
        if (chunks === null) {
          yield encoder.encode("-END");
          continue;
        }
        yield* chunks;
      }
    })();
  },
};

assert.strictEqual(
  decode(bytesSync(pullSync(fromSync("data"), stateful))),
  "data-END",
);

const add = (suffix) => (chunks) =>
  chunks === null ? null : [...chunks, encoder.encode(suffix)];
assert.strictEqual(
  decode(bytesSync(pullSync(fromSync("hello"), add("!"), add("?")))),
  "hello!?",
);

assert.throws(() => pullSync(fromSync("x"), { transform: 1 }), {
  code: "ERR_INVALID_ARG_TYPE",
});
