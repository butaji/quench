function sum(values) {
  var total = 0;
  for (var value of values) {
    total += value;
  }
  return total;
}

sum([1, 2]);
var result = sum([10, 11, 21]);
if (result !== 42) {
  throw new Error("packed for-of sum mismatch");
}
return result;
