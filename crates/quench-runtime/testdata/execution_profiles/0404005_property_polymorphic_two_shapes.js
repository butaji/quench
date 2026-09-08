function read(object) {
  return object.value + 1;
}

var compact = { value: 40 };
var extended = { prefix: 7, value: 41 };
read(compact);
read(extended);
function run() {
  return read(compact) + read(extended);
}
function verify(result) {
if (result !== 83) {
  throw new Error("bounded polymorphic property mismatch");
}
  return result;
}
return { run: run, verify: verify };
