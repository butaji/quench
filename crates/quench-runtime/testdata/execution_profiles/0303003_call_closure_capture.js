function makeAdder(base) {
  return function add(value) {
    return base + value;
  };
}

var add = makeAdder(10);
add(1);
var result = add(32);
if (result !== 42) {
  throw new Error("closure capture call mismatch");
}
return result;
