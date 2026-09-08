function choose(value, fallback) {
  return value ?? (fallback ? 41 : 7);
}

choose(1, false);
var zero = choose(0, true);
var missing = choose(null, true);
function run() {
  return zero + missing;
}
function verify(result) {
if (result !== 41) {
  throw new Error("nullish/truthy branch mismatch");
}
  return result;
}
return { run: run, verify: verify };
