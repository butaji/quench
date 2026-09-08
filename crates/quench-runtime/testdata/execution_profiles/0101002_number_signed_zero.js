function negate(value) {
  return value * -1;
}

negate(1);
function verify(result) {
if (!Object.is(result, -0)) {
  throw new Error("signed zero was not preserved");
}
  return result;
}
return { run: negate, arguments: [0], verify: verify };
