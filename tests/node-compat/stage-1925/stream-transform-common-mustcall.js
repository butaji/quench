const common = require("../../node/common");
const assert = require("assert");
const stream = require("stream");

const t = new stream.Transform({
  autoDestroy: true,
  transform(data, enc, cb) {
    cb(null, data);
  },
  destroy: common.mustCall((err, cb) => cb()),
});
let ended = false;
let finished = false;
t.write("hello");
t.write("world");
t.end();
t.resume();
t.on(
  "end",
  common.mustCall(() => (ended = true)),
);
t.on(
  "finish",
  common.mustCall(() => (finished = true)),
);
t.on(
  "close",
  common.mustCall(() => {
    assert(ended);
    assert(finished);
    console.log("transform common.mustCall passed");
  }),
);
