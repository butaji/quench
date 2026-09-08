function scale(values, factor) {
  var sum = 0;
  for (var i = 0; i < values.length; i++) {
    values[i] = values[i] * factor;
    sum = sum + values[i];
  }
  return sum;
}

scale(new Float64Array([1, 2]), 2);
var values = new Float64Array([1.5, -2, 4, 0.5]);
function run() {
  return scale(values, 2);
}
function verify(result) {
if (result !== 8 || values[0] !== 3 || values[1] !== -4 || values[2] !== 8) {
  throw new Error("typed-array f64 loop mismatch");
}
  return result;
}
return { run: run, verify: verify };
