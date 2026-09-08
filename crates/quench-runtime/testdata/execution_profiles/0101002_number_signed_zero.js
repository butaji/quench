function negate(value) {
  return value * -1;
}

negate(1);
var result = negate(0);
if (!Object.is(result, -0)) {
  throw new Error("signed zero was not preserved");
}
return result;
