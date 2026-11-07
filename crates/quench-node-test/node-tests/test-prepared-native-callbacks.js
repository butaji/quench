// A native's loop of callback calls (array iteration, sort comparators, collection forEach)
// runs its callee as Node does: receivers, arguments, legacy reflection, re-entrancy, throws,
// collections, mutation of the array and of the callee's binding, non-lane callees.
var gc = typeof $262 !== "undefined" ? $262.gc : typeof globalThis.gc === "function" ? globalThis.gc : function () {};
var out = [];
function run() {
  // this, arguments and legacy f.arguments for sloppy, strict and arrow callbacks
  var a = [1, 2, 3];
  function sloppy(x) { out.push(typeof this, this === globalThis, arguments.length, sloppy.arguments[2] === a); }
  a.forEach(sloppy);
  function strict(x) { "use strict"; out.push(this === undefined, arguments.length); }
  a.forEach(strict);
  var receiver = { k: 9 };
  a.forEach(function (x) { out.push(this.k + x); }, receiver);
  a.forEach(function () { "use strict"; out.push(typeof this); }, 5);
  a.forEach(function () { out.push(typeof this); }, 5);
  a.forEach((x, i, arr) => out.push(x * 10 + i, arr === a));
  // re-entrancy: a callback that runs forEach again with the same and another callee
  var depth = 0;
  function nest(x) { if (depth < 2) { depth++; [x, x + 1].forEach(nest); depth--; } out.push("n" + depth + x); }
  [1, 2].forEach(nest);
  // a throw propagates and later calls do not run
  try { a.forEach(function (x) { out.push("t" + x); if (x === 2) throw new Error("stop" + x); }); }
  catch (e) { out.push(e.message); }
  // the array mutates under the loop: pushes are not visited, removals are skipped
  var m = [1, 2, 3, 4];
  m.forEach(function (x, i) { out.push("m" + x); if (i === 0) { m.push(99); m.pop(); m.pop(); } });
  var holes = [1, , 3]; holes.forEach(function (x, i) { out.push("h" + i); if (i === 0) holes[1] = 7; });
  // the callee's binding is redefined: the loop keeps the function it was given
  var g = function (x) { out.push("g" + x); g = function () { out.push("never"); }; };
  a.forEach(g);
  // collections in callbacks: values the native keeps after the call stay alive
  var objs = []; for (var i = 0; i < 50; i++) objs.push({ v: i });
  var kept = objs.filter(function (o, i) { o = null; objs[i] = null; if (i % 7 === 0) gc(); var junk = []; for (var j = 0; j < 100; j++) junk.push({ j: j }); return i % 5 === 0; });
  out.push(kept.map(function (o) { return o.v; }).join("."));
  out.push([5, 6, 7].map(function (x) { gc(); return { y: x }; }).map((o) => o.y).join("."));
  out.push([1, 2, 3].some((x) => x > 1), [1, 2, 3].every((x) => x > 1), [1, 2, 3].find((x) => x > 1), [1, 2, 3].findIndex((x) => x > 5), [1, 2, 3].findLast((x) => x < 3), [1, 2, 3].findLastIndex((x) => x < 3));
  // sort comparators: the same exactness
  var s = [5, 1, 4, 2, 3];
  out.push(s.slice().sort(function (x, y) { gc(); return x - y; }).join(""));
  try { s.slice().sort(function (x, y) { if (x === 4 || y === 4) throw new Error("cmp"); return x - y; }); } catch (e) { out.push(e.message); }
  out.push(s.slice().sort(function (x, y) { "use strict"; return this === undefined ? y - x : 0; }).join(""));
  out.push(s.slice().sort((x, y) => arguments.length + x - y).join(""));
  // a class constructor is not callable, a bound function and a proxy are
  try { a.forEach(class K {}); } catch (e) { out.push(e.constructor.name); }
  a.forEach(function (x) { out.push(this.k * x); }.bind({ k: 2 }));
  a.forEach(new Proxy(function (x) { out.push("p" + x); }, {}));
  // a generator and an async callback
  a.forEach(function* (x) { out.push("never"); });
  a.forEach(async function (x) { out.push("a" + x); await null; out.push("late" + x); });
  // deep recursion through a callback reaches a RangeError, which unwinds
  function down(n) { [n].forEach(function (k) { down(k + 1); }); }
  try { down(0); } catch (e) { out.push(e.constructor.name); }
  // a callback that redefines Array.prototype.forEach while it runs
  var original = Array.prototype.forEach;
  [1, 2].forEach(function (x) { Array.prototype.forEach = function () { out.push("replaced"); }; out.push("o" + x); });
  [1].forEach(function () {});
  Array.prototype.forEach = original;
  // reducers and collection walks
  out.push([1, 2, 3, 4].reduce(function (acc, x, i, arr) { return acc + x * i + arr.length + arguments.length; }, 0));
  out.push([1, 2, 3].reduceRight((acc, x) => acc + "" + x, "r"));
  var map = new Map([[1, "a"], [2, "b"]]);
  map.forEach(function (v, k, m) { out.push(k + v + (m === map) + (this === receiver)); if (k === 1) { map.delete(2); map.set(3, "c"); } }, receiver);
  var set = new Set([1, 2]);
  set.forEach(function (v, w) { out.push(v === w, typeof this); if (v === 1) set.add(5); gc(); });
  // many elements, so the loop's later calls run as its first did
  var sum = 0; var big = []; for (var i = 0; i < 3000; i++) big.push(i);
  big.forEach(function (x) { sum += x; if (x % 1000 === 0) gc(); });
  out.push(sum);
}
run();
run();
Promise.resolve().then(function () { console.log(out.join(",")); });
