function parse(input) {
  var match = /^(a+)(b+)\1$/.exec(input);
  return match === null ? "missing" : match[1] + ":" + match[2];
}

parse("aba");
function run() {
  return parse("aaabbbaaa");
}
function verify(result) {
if (result !== "aaa:bbb") {
  throw new Error("regexp capture/backtrack mismatch: " + result);
}
  return result;
}
return { run: run, verify: verify };
