function update(source, target, coefficient) {
  for (var i = 1; i < source.length - 1; i++) {
    target[i] = source[i] + coefficient * (source[i - 1] + source[i + 1]);
  }
  return target[1] + target[2] + target[3];
}

update([1, 2, 3], [0, 0, 0], 0.5);
var result = update([1, 2, 4, 8, 16], [0, 0, 0, 0, 0], 0.25);
if (result !== 22.75) {
  throw new Error("dense neighbor update mismatch: " + result);
}
return result;
