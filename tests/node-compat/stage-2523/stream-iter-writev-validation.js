const assert = require("assert");
const { Writable } = require("stream");
const { fromWritable } = require("stream/iter");

const writer = fromWritable(
  new Writable({
    write(_chunk, _encoding, callback) {
      callback();
    }
  })
);
assert.throws(() => writer.writev("no"), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => writer.writev([new Uint8Array(1), 42]), {
  code: "ERR_INVALID_ARG_TYPE"
});
assert.throws(() => fromWritable(new Writable({ objectMode: true })), {
  code: "ERR_INVALID_STATE"
});
