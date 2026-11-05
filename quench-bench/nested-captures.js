// Standalone composition probe; the frozen microbenchmark edition stays separate.
function nestedMutable(n, seed) {
  var outer = seed;
  function makeMiddle() {
    var middle = outer + 1;
    return function makeInner() {
      var inner = middle + 1;
      return function () {
        return ++outer + ++middle + ++inner;
      };
    };
  }
  var makeInner = makeMiddle();
  var first = makeInner();
  var second = makeInner();
  var total = 0;
  for (var i = 0; i < n; i++) total += (i % 2 === 0 ? first : second)();
  return total;
}

for (var n of [0, 1, 2, 17, 1000]) {
  for (var seed of [0, 7, -3]) {
    console.log(n, seed, nestedMutable(n, seed));
  }
}
