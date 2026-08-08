const assert = require("assert");
const { Readable } = require("stream");

const naturals = () =>
  Readable.from(
    (async function* () {
      let value = 1;
      while (true) yield value++;
    })()
  );

let resolved = false;
Promise.all([
  naturals().take("cat").toArray(),
  naturals().take("2").toArray(),
  naturals().take(true).toArray()
]).then((values) => {
  assert.deepStrictEqual(values, [[], [1, 2], [1]]);
  resolved = true;
});

process.on("beforeExit", () => {
  assert.strictEqual(resolved, true);
  assert.throws(() => Readable.from([]).take(-1), {
    code: "ERR_OUT_OF_RANGE"
  });
});
