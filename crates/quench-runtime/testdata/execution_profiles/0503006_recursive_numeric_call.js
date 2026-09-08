function factorial(value) {
  if (value <= 1) return 1;
  return value * factorial(value - 1);
}

factorial(3);
var result = factorial(5);
if (result !== 120) {
  throw new Error("recursive numeric call mismatch");
}
return result;
