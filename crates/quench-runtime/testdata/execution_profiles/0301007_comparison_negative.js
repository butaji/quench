function classify(value) {
  if (value !== value) return 1;
  if (value === 0 && 1 / value === -Infinity) return 2;
  if (value < 0) return 3;
  return 4;
}

classify(1);
function verify(result) {
  if (classify(NaN) !== 1) throw new Error("NaN classification mismatch");
  if (classify(-0) !== 2) throw new Error("negative-zero classification mismatch");
  if (classify(-1) !== 3) throw new Error("negative classification mismatch");
  if (classify(1) !== 4) throw new Error("positive classification mismatch");
  if (result !== 3) throw new Error("measured negative classification mismatch");
  return result;
}
return { run: classify, verify: verify, arguments: [-1] };
