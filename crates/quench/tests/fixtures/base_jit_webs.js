// Shared temporaries: t holds an int index, then a double element, then an int again, across exits.
function mix(a, b, n, seed) {
  var s = 0, t, u, k = 0, out = 0;
  for (var i = 0; i < n; i++) {
    t = (i * 3 + seed) & 63;          // int
    u = a[t];                          // double element (or int)
    t = u * 0.5 + t;                   // double reuse of t
    s = s + t;                         // double sum
    t = (i + 7) | 0;                   // int again
    b[t & 63] = s;                     // store with int index, double value
    k = (k + t) | 0;                   // int accumulator
    if (u > 40) { out = out + 1; }     // compare on double
    t = a[k & 63];              // index from int, element reuse
    out = out + (t | 0);
  }
  return s + k + out;
}
function build(n, kind) {
  var a = [];
  for (var i = 0; i < n; i++) a.push(kind === 0 ? i * 0.75 : (kind === 1 ? i : (i % 5 === 0 ? i + 0.5 : i)));
  return a;
}
var res = [];
for (var rep = 0; rep < 400; rep++) {
  var kind = rep % 3, a = build(64, kind), b = build(64, 1);
  res.push(mix(a, b, 200 + (rep % 17), rep % 11));
  if (rep % 50 === 7) { a[(rep * 7) % 64] = "x"; res.push(String(mix(a, b, 150, 3))); }   // non-number: forces exits
  if (rep % 60 === 13) { a[5] = undefined; res.push(String(mix(a, b, 120, 2))); }
  if (rep % 70 === 21) { a.length = 30; res.push(String(mix(a, b, 100, 1))); }          // out of range
}
var sum = 0; for (var i = 0; i < res.length; i++) sum = (sum * 31 + String(res[i]).length + (+res[i] | 0)) | 0;
console.log(res.length, sum, res[0], res[100], res[res.length - 1]);
function ints(n) {   // int counters only; overflow exit at the end
  var c = 0, d = 2147483000, e;
  for (var i = 0; i < n; i++) { e = i & 15; c = (c + e) | 0; d = d + e; }
  return c + ":" + d;
}
for (var r = 0; r < 60; r++) var last = ints(3000);
console.log(last);
