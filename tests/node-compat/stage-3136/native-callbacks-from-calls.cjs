// Array and string natives called from inside functions call JavaScript back: the values,
// receivers, arguments, errors and stack depth each callback sees, repeated so the calling
// functions run hot.
const assert = require('assert');

function nest(n) {
  return [n].map((v) => {
    if (v === 0) throw new TypeError('bottom');
    try {
      return nest(v - 1);
    } catch (error) {
      if (v % 3 === 0) return 'c' + v;
      throw error;
    }
  })[0];
}

function check() {
  const add = (x, y) => x + y;
  assert.strictEqual([1, 2, 3, 4, 5].reduce(add, 0), 15);
  assert.strictEqual([1, 2, 3].reduce(function (x) { return x + arguments.length; }, 0), 12);
  assert.strictEqual([1, 2, 3].reduce((x, ...rest) => x + rest.length, 0), 9);
  assert.strictEqual([1].map(function () { return this; }, { t: 1 })[0].t, 1);
  assert.strictEqual([1].map(function () { 'use strict'; return typeof this; })[0], 'undefined');
  assert.strictEqual([1].map(function () { return this === globalThis; })[0], true);
  assert.strictEqual([1].map(function () { return typeof this; }, 5)[0], 'object');
  assert.strictEqual([1, 2, 3].map(function f() { return f.arguments.length; }).join(), '3,3,3');
  assert.strictEqual([1].map(function (x) { arguments[0] = 9; return x; })[0], 9);
  assert.strictEqual([1].map((a, b, c, d, e) => [typeof a, typeof b, typeof c, typeof d, typeof e].join('/'))[0],
    'number/number/object/undefined/undefined');
  assert.throws(() => [1, 2, 3].forEach((x) => { if (x === 2) throw new RangeError('boom' + x); }), /boom2/);
  assert.strictEqual([1, 2, 3, 4, 5].map((x) => { try { if (x & 1) throw x; return 0; } catch (e) { return -e; } }).join(),
    '-1,0,-3,0,-5');
  assert.strictEqual(nest(7), 'c3');
  assert.throws(() => nest(2), /bottom/);
  let finished = 0;
  assert.throws(() => [1, 2, 3].forEach((x) => { try { if (x === 2) throw 1; } finally { finished += x; } }));
  assert.strictEqual(finished, 3);
  assert.strictEqual([1, 2, 3].map((x) => () => x * 10).map((f) => f()).join(), '10,20,30');
  const id = (x) => x;
  const twice = (x) => id(id(x) * 2);
  assert.strictEqual([1, 2].map((x) => twice(x)).join(), '2,4');
  assert.strictEqual([[3, 1, 2], [9, 7]].map((r) => r.slice().sort((a, b) => a - b).join('')).join('|'), '123|79');
  assert.strictEqual(['a1b2', 'c3'].map((s) => s.replace(/\d/g, (d) => String(d * 2))).join(), 'a2b4,c6');
  assert.strictEqual([null].map((v) => { try { return v.x; } catch (e) { return e.constructor.name; } })[0], 'TypeError');
  const fib = (n) => (n < 2 ? n : [n - 1, n - 2].map(fib).reduce((a, b) => a + b));
  assert.strictEqual(fib(12), 144);
  class Point { constructor(v) { this.v = v; } get double() { return this.v * 2; } }
  assert.strictEqual([1, 2].map((v) => new Point(v)).map((p) => p.double).join(), '2,4');
  assert.throws(() => [1].map(Point), TypeError);
  assert.strictEqual([1].map(function* () { yield 1; })[0].next().value, 1);
  assert.strictEqual([1, 2].map(function (x) { return x + this.k; }.bind({ k: 10 })).join(), '11,12');
  assert.strictEqual(JSON.stringify({ a: 1, b: [2] }, (k, v) => (typeof v === 'number' ? v + 1 : v)), '{"a":2,"b":[3]}');
  let collected = 0;
  const sum = Array.from({ length: 2000 }, (_, i) => i).reduce((s, v) => { const o = { v, s: [v, v] }; collected += o.s.length; return s + o.v; }, 0);
  assert.strictEqual(sum, 1999000);
  assert.strictEqual(collected, 4000);
  return [1, 2].map(async (x) => { await null; return x * 3; });
}

function forever(n) { return [n].map(forever); }

(async () => {
  for (let round = 0; round < 50; round++) {
    assert.deepStrictEqual(await Promise.all(check()), [3, 6]);
  }
  assert.throws(() => forever(0), RangeError);
  with ({ q: 1 }) { assert.strictEqual([1].map((x) => x + q)[0], 2); }
  console.log('ok');
})();
