function read(object) {
  return object.value;
}

var receiver = { value: 19 };
read(receiver);
function verify(result) {
if (result !== 19) {
  throw new Error("monomorphic property assertion failed: " + result);
}
  return result;
}
return { run: read, arguments: [receiver], verify: verify };
