function classify(value) {
  if (value !== value) return 1;
  if (Object.is(value, -0)) return 2;
  if (value < 0) return 3;
  return 4;
}

classify(1);
var result = classify(NaN) * 100 + classify(-0) * 10 + classify(-1);
if (result !== 123) {
  throw new Error("comparison edge classification mismatch");
}
return result;
