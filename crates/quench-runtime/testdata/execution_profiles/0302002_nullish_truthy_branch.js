function choose(value, fallback) {
  return value ?? (fallback ? 41 : 7);
}

choose(1, false);
var zero = choose(0, true);
var missing = choose(null, true);
var result = zero + missing;
if (result !== 41) {
  throw new Error("nullish/truthy branch mismatch");
}
return result;
