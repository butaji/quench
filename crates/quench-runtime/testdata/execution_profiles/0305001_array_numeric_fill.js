function fill(values, value) {
  for (var i = 0; i < values.length; i++) values[i] = value;
  return values[0] + values[values.length - 1];
}

fill([0, 0], 1);
var values = [0, 0, 0, 0, 0];
function run() {
  return fill(values, 21);
}
function verify(result) {
if (result !== 42 || values.join(",") !== "21,21,21,21,21") {
  throw new Error("numeric fill mismatch");
}
  return result;
}
return { run: run, verify: verify };
