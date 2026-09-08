function fill(values, value) {
  for (var i = 0; i < values.length; i++) values[i] = value;
  return values[0] + values[values.length - 1];
}

var values = [1, 2, 3];
function verify(result) {
  if (result === result || values[1] === values[1]) throw new Error("NaN fill mismatch");
  return result;
}
return { run: fill, verify: verify, arguments: [values, NaN] };
