// Call arguments computed ahead of other arguments end in register moves that the compiler
// folds into their producers, spans nested and chained within one function; values held only
// in registers across allocating calls stay alive through collections.
const assert = require("assert");

function pair(a, b) { return [a, b]; }
function tri(a, b, c) { return a + "/" + b + "/" + c; }
function obj(k) { return { k, s: "v" + k }; }

function nested(i) {
  const x = i * 2, y = "y" + i;
  return tri(pair(x + 1, obj(i).s)[1], tri(y, pair(i, x)[0], obj(x).k), pair(tri(i, i + 1, y), x - 1).join(":"));
}
assert.deepStrictEqual([0, 1, 2, 3].map(nested), [
  "v0/y0/0/0/0/1/y0:-1",
  "v1/y1/1/2/1/2/y1:1",
  "v2/y2/2/4/2/3/y2:3",
  "v3/y3/3/6/3/4/y3:5",
]);

function held(n) {
  const out = [];
  for (let i = 0; i < n; i++) {
    const a = obj(i), b = obj(i + 1).s, c = pair(a, b);
    out.push(tri(c[0].s, pair(b, new Array(64).fill(i)).length, c[1]));
  }
  return out;
}
const h = held(30000);
assert.strictEqual(h.length, 30000);
assert.strictEqual(h[0], "v0/2/v1");
assert.strictEqual(h[12345], "v12345/2/v12346");
assert.strictEqual(h[29999], "v29999/2/v30000");

const swapped = (function (a, b) { const t = a; a = b; b = t; return tri(a, b, t); })("p", "q");
assert.strictEqual(swapped, "q/p/p");
console.log("ok");
