const assert = require("assert");

const legacy = {
  refCalled: 0,
  unrefCalled: 0,
  ref() {
    this.refCalled += 1;
  },
  unref() {
    this.unrefCalled += 1;
  },
};
const symbolic = {
  refCalled: 0,
  unrefCalled: 0,
  [Symbol.for("nodejs.ref")]() {
    this.refCalled += 1;
  },
  [Symbol.for("nodejs.unref")]() {
    this.unrefCalled += 1;
  },
};

process.ref(legacy);
process.unref(legacy);
process.ref(symbolic);
process.unref(symbolic);
assert.deepStrictEqual(legacy, { refCalled: 1, unrefCalled: 1 });
assert.strictEqual(symbolic.refCalled, 1);
assert.strictEqual(symbolic.unrefCalled, 1);

const timer = setInterval(() => {}, 1000);
assert.strictEqual(timer.hasRef(), true);
process.unref(timer);
assert.strictEqual(timer.hasRef(), false);
process.ref(timer);
assert.strictEqual(timer.hasRef(), true);
clearInterval(timer);
