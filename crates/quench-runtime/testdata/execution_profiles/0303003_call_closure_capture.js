function makeAdder(base) {
  return function add(value) {
    return base + value;
  };
}

var add = makeAdder(10);
add(1);
function run() {
  return add(32);
}
function verify(result) {
if (result !== 42) {
  throw new Error("closure capture call mismatch");
}
  return result;
}
return { run: run, verify: verify };
