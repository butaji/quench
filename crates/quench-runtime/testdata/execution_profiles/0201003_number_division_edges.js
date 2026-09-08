function divide(left, right) { return left / right; }

divide(4, 2);
function run() {
  var positive = divide(1, 0);
  var negative = divide(-1, 0);
  var nan = divide(0, 0);
  return positive === Infinity && negative === -Infinity && Number.isNaN(nan);
}
function verify(result) {
if (!result) {
  throw new Error("number division edge mismatch");
}
  return result;
}
return { run: run, verify: verify };
