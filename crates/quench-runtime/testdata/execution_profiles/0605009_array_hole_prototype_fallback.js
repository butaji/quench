function read(values) {
  return values[1];
}

Array.prototype[1] = 41;
var values = [0, , 2];
function run() {
  return read(values);
}
function verify(result) {
delete Array.prototype[1];
if (result !== 41) {
  throw new Error("array hole prototype fallback mismatch");
}
  return result;
}
return { run: run, verify: verify };
