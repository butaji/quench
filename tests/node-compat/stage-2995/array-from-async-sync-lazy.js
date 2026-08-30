"use strict";

const assert = require("assert");

let nextCalls = 0;
let closed = 0;
const iterable = {
  [Symbol.iterator]() {
    return {
      next() {
        nextCalls += 1;
        return { value: nextCalls, done: false };
      },
      return() {
        closed += 1;
        return { done: true };
      }
    };
  }
};

(async () => {
  await assert.rejects(
    Array.fromAsync(iterable, async (value) => {
      assert.strictEqual(value, 1);
      throw new Error("stop");
    }),
    /stop/
  );
  assert.strictEqual(nextCalls, 1);
  assert.strictEqual(closed, 1);

  function CustomArray() {}
  const custom = await Array.fromAsync.call(
    CustomArray,
    [1, 2],
    async (value) => value * 2
  );
  assert.deepStrictEqual([custom[0], custom[1]], [2, 4]);

  let second = 2;
  const arrayLike = {
    length: 2,
    0: 1,
    get 1() {
      return second;
    }
  };
  const changed = await Array.fromAsync(arrayLike, async (value) => {
    if (value === 1) second = 3;
    return value;
  });
  assert.deepStrictEqual(changed, [1, 3]);

  let thenReads = 0;
  const thenable = {
    get then() {
      thenReads += 1;
      return (resolve) => resolve(7);
    }
  };
  const assimilated = await Array.fromAsync([thenable], (value) => ({
    then(resolve) {
      resolve(value + 1);
    }
  }));
  assert.deepStrictEqual(assimilated, [8]);
  assert.strictEqual(thenReads, 1);
  console.log("ok");
})();
