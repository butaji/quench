function calculate(a, b) {
  var x0 = a + 1;
  var x1 = b + 2;
  var x2 = x0 * x1;
  var x3 = x2 - x0;
  var x4 = x3 + x1;
  var x5 = x4 * 2;
  return x5 - x2;
}

calculate(1, 2);
function run() {
  return calculate(3, 4);
}
function verify(result) {
if (result !== 28) {
  throw new Error("many-local arithmetic mismatch: " + result);
}
  return result;
}
return { run: run, verify: verify };
