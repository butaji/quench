function reduce(values) {
  var total = 0;
  for (var i = 0; i < values.length; i++) total = total + values[i];
  return total;
}

reduce([1, 2]);
function run() {
  return reduce([1e16, 1, -1e16, 42]);
}
function verify(result) {
if (result !== 42) {
  throw new Error("ordered reduction mismatch: " + result);
}
  return result;
}
return { run: run, verify: verify };
