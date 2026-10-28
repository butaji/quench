var first = [];
var second = [];
for (var index = 0; index < 500000; index++) {
  first[index] = index;
  second[index] = index;
}
if (first[499999] !== 499999 || second[499999] !== 499999) {
  throw new Error("indexed fill mismatch");
}
