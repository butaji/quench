function update(values) {
  for (var i = 1; i < values.length; i++) {
    values[i] = values[i] + values[i - 1];
  }
  return values[3];
}

update([1, 1]);
var values = [1, 2, 3, 4];
function run() {
  return update(values);
}
function verify(result) {
if (result !== 10 || values.join(",") !== "1,3,6,10") {
  throw new Error("ordered alias update mismatch");
}
  return result;
}
return { run: run, verify: verify };
