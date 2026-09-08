function mix(words) {
  var state = 0;
  for (var i = 0; i < words.length; i++) {
    state = (((state << 5) - state) ^ words[i]) | 0;
  }
  return state;
}

mix([1, 2]);
function run() {
  return mix([305419896, -1, 324508639, 610839776]);
}
function verify(result) {
if (result !== -1155123478) {
  throw new Error("integer limb mix mismatch: " + result);
}
  return result;
}
return { run: run, verify: verify };
