function negate(value) {
  return value * -1;
}

negate(1);
function run() {
  return negate(0);
}
function verify(result) {
if (!Object.is(result, -0)) {
  throw new Error("signed zero was not preserved");
}
  return result;
}
return { run: run, verify: verify };
