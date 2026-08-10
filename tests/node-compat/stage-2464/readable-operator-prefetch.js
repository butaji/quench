const assert = require("assert");
const { Readable } = require("stream");

const consumeFour = async (stream) => {
  let consumed = 0;
  for await (const value of stream) {
    assert.strictEqual(value, 1);
    consumed++;
    if (consumed === 4) break;
  }
};

let mapCalls = 0;
const mapped = Readable.from(
  (async function* () {
    while (true) yield 1;
  })(),
).map((value) => {
  mapCalls++;
  return value;
});

let filterCalls = 0;
const filtered = Readable.from(
  (async function* () {
    while (true) yield 1;
  })(),
).filter((value) => {
  filterCalls++;
  return value === 1;
});

let completed = false;
Promise.all([consumeFour(mapped), consumeFour(filtered)]).then(() => {
  assert.strictEqual(mapCalls, 5);
  assert.strictEqual(filterCalls, 5);
  completed = true;
});

process.on("beforeExit", () => {
  assert.strictEqual(completed, true);
});
