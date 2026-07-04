// spec: ecma-262 sec-arguments-exotic-objects
// expect: value: 3
// tags: arguments, functions

function f() {
  return arguments.length;
}
f(1, 2, 3);
