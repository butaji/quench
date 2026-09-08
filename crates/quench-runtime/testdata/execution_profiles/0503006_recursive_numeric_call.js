function factorial(value) {
  if (value <= 1) return 1;
  return value * factorial(value - 1);
}

factorial(3);
function run() {
  return factorial(5);
}
function verify(result) {
if (result !== 120) {
  throw new Error("recursive numeric call mismatch");
}
  return result;
}
return { run: run, verify: verify };
