function read(object) {
  return object.value;
}

var receiver = { value: 19 };
read(receiver);
function run() {
  return read(receiver);
}
function verify(result) {
if (result !== 19) {
  throw new Error("monomorphic property assertion failed: " + result);
}
  return result;
}
return { run: run, verify: verify };
