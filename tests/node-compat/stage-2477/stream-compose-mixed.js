const assert = require("assert");
const { compose, Transform, Writable } = require("stream");

let consumed = "";
let finished = false;
let errorSeen = false;
const keepAlive = setInterval(() => {}, 10);
const complete = () => {
  if (finished && errorSeen) clearInterval(keepAlive);
};

compose(
  new Transform({
    transform(chunk, _encoding, callback) {
      callback(null, chunk.toString().toUpperCase());
    }
  }),
  async function* (source) {
    yield* source;
  },
  new Writable({
    write(chunk, _encoding, callback) {
      consumed += chunk;
      callback();
    }
  })
)
  .end("value")
  .on("finish", () => {
    assert.strictEqual(consumed, "VALUE");
    finished = true;
    complete();
  });

const failure = new Error("mixed compose failed");
compose(
  new Transform({
    objectMode: true,
    transform(_chunk, _encoding, callback) {
      callback(failure);
    }
  }),
  async function* (source) {
    yield* source;
  }
)
  .end(true)
  .on("error", (error) => {
    assert.strictEqual(error, failure);
    errorSeen = true;
    complete();
  });

process.on("beforeExit", () => {
  assert.strictEqual(finished, true);
  assert.strictEqual(errorSeen, true);
});
