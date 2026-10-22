function sumTo(n, total) {
  'use strict';
  if (n === 0) return total;
  return sumTo(n - 1, total + n);
}
function increment(value) { return value + 1; }
var ordinary = 0;
for (var index = 0; index < 1000; index++) ordinary = increment(ordinary);
console.log(ordinary, sumTo(500, 0));
