function affine(state) {
  var value = state.seed;
  for (var index = 0; index < state.n; index++) {
    value = (value * 33 + 7) | 0;
  }
  return value;
}

var result = affine({ seed: 1, n: 4 });
if (result !== 1445341) {
  throw new Error("affine loop assertion failed: " + result);
}
return result;
