const assert = require("assert");
const { Readable } = require("stream");

const shortCircuit = Readable.from([1, 2, 3, 4, 5]);
Object.defineProperty(shortCircuit, "map", {
  value: () => assert.fail("terminal predicates must not call public map"),
});

const controller = new AbortController();
const aborted = Readable.from([1, 2, 3]).some(() => new Promise(() => {}), {
  signal: controller.signal,
});

let failure;
const cases = [
  shortCircuit
    .find((value) => value > 2)
    .then((value) => {
      assert.strictEqual(shortCircuit.destroyed, true);
      return value;
    }),
  Readable.from([1, 2, 3]).some(async (value) => value === 2),
  Readable.from([1, 2, 3]).every(async (value) => value < 4),
  Readable.from([]).some(() => true),
  Readable.from([]).every(() => false),
  Readable.from([]).find(() => true),
  assert.rejects(aborted, { name: "AbortError" }),
  assert.rejects(Readable.from([1]).some(1), {
    code: "ERR_INVALID_ARG_TYPE",
  }),
];
const expected = [3, true, true, false, true, undefined];
let completed = 0;
const keepAlive = setInterval(() => {}, 10);
for (let index = 0; index < cases.length; index++) {
  Promise.resolve(cases[index]).then(
    (value) => {
      try {
        if (index < expected.length) {
          assert.strictEqual(value, expected[index]);
        }
      } catch (error) {
        failure ||= error;
      }
      completed++;
      if (completed === cases.length) clearInterval(keepAlive);
    },
    (error) => {
      failure ||= error;
      completed++;
      if (completed === cases.length) clearInterval(keepAlive);
    },
  );
}

controller.abort();

process.on("beforeExit", () => {
  if (failure) throw failure;
  assert.strictEqual(completed, cases.length);
});
