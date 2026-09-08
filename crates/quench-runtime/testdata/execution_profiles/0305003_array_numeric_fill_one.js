function fill(values, value) {
  for (var i = 0; i < values.length; i++) values[i] = value;
  return values[0] + values[values.length - 1];
}

var values = [0];
function verify(result) {
  if (result !== 42 || values[0] !== 21) throw new Error("one fill mismatch");
  return result;
}
return { run: fill, verify: verify, arguments: [values, 21] };
