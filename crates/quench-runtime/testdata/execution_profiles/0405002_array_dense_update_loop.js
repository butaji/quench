function update(values, delta) {
  for (var i = 0; i < values.length; i++) {
    values[i] = values[i] + delta;
  }
  return values[0] + values[1] + values[2] + values[3];
}

update([0, 1, 2, 3], 1);
function run() {
  return update([10, 20, 30, 40], 0.5);
}
function verify(result) {
if (result !== 102) {
  throw new Error("dense array update mismatch");
}
  return result;
}
return { run: run, verify: verify };
