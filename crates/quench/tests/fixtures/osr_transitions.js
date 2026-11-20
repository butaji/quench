// One hot loop entered cold (interpreter to region at its header), run long enough for a
// better tier to publish while an earlier tier's region is running, left at guards (an int
// overflow into doubles, a shape change, a hole) and entered again: every phase prints the
// same lines whatever tier ran each stretch of it.
function walk(items, n, flip) {
  var sum = 0, last = 0;
  for (var i = 0; i < n; i++) {
    var item = items[i & 7];
    sum = (sum + item.v + (i === flip ? 0.5 : 0)) % 1000003;
    if (i === flip) item.w = i;
    last = item.w === undefined ? last : item.w;
  }
  return sum + ":" + last;
}
function make() {
  var items = [];
  for (var k = 0; k < 8; k++) items.push({ v: k * 3, w: undefined });
  return items;
}
console.log("cold " + walk(make(), 5000, -1));
console.log("long " + walk(make(), 3000000, -1));
for (var rep = 0; rep < 400; rep++) walk(make(), 200, -1);
console.log("body " + walk(make(), 3000000, -1));
console.log("guard " + walk(make(), 3000000, 1500000));
console.log("shape " + walk(make().map(function (item, k) { return k === 3 ? { w: undefined, v: 9 } : item; }), 200000, 100000));
console.log("again " + walk(make(), 3000000, -1));
