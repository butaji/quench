function read(object) {
  return object.value;
}

var receiver = { value: 19 };
read(receiver);
var result = read(receiver);
if (result !== 19) {
  throw new Error("monomorphic property assertion failed: " + result);
}
return result;
