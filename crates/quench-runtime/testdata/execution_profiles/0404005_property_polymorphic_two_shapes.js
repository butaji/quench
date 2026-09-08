function read(object) {
  return object.value + 1;
}

var compact = { value: 40 };
var extended = { prefix: 7, value: 41 };
read(compact);
read(extended);
var left = read(compact);
var right = read(extended);
function run() {
  return left + right;
}
function verify(result) {
if (result !== 83) {
  throw new Error("bounded polymorphic property mismatch");
}
  return result;
}
return { run: run, verify: verify };
