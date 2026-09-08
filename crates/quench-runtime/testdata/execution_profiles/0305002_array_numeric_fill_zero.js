function fill(values, value) {
  for (var i = 0; i < values.length; i++) values[i] = value;
  return values[0] + values[values.length - 1];
}

var values = [];
function verify(result) {
  if (result === result || values.length !== 0) throw new Error("zero fill mismatch");
  return result;
}
return { run: fill, verify: verify, arguments: [values, 21] };
