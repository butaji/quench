function makeAdder(base) {
  return function add(value) {
    return base + value;
  };
}

var add = makeAdder(10);
add(1);
function verify(result) {
if (result !== 42) {
  throw new Error("closure capture call mismatch");
}
  return result;
}
return { run: add, arguments: [32], verify: verify };
