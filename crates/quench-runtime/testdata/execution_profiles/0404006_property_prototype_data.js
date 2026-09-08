function read(object) {
  return object.value + 1;
}

var prototype = { value: 41 };
var receiver = Object.create(prototype);
read(receiver);
function run() {
  return read(receiver);
}
function verify(result) {
if (result !== 42) {
  throw new Error("prototype data read mismatch");
}
  return result;
}
return { run: run, verify: verify };
