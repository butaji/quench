function add(value) { return value + 1; }
function multiply(value) { return value * 2; }
function invoke(fn, value) { return fn(value); }

invoke(add, 1);
invoke(multiply, 1);
var result = invoke(add, 20) + invoke(multiply, 10);
if (result !== 41) {
  throw new Error("polymorphic call mismatch");
}
return result;
