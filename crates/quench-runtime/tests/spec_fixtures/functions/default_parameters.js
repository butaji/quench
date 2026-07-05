// spec: ECMA-262 sec-function-definitions-static-semantics-early-errors
// expect: value: 5
// tags: functions, defaults

function greet(name, greeting = "Hello") {
  return greeting + " " + name;
}
greet("World");
