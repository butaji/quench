"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("list assertion failed: " + message);
}
function cons(value, next) {
  return { value: value, next: next };
}
var list = null;
for (var i = 8; i >= 1; i--) list = cons(i, list);
var result = 0;
while (list !== null) {
  result += list.value;
  list = list.next;
}
assert(result === 36, "ordered tagged list sum");
return result;
