function read(object) {
  return object.value;
}

var prototype = { value: 41 };
var receiver = Object.create(prototype);
read(receiver);
read(receiver);
receiver.value = 42;
function run() {
  return read(receiver);
}
function verify(result) {
if (result !== 42) {
  throw new Error("prototype shadow fallback mismatch");
}
  return result;
}
return { run: run, verify: verify };
