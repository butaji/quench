"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("regexp scan assertion failed: " + message);
}
var expression = /\d+/g;
var input = "alpha=12 beta=345 gamma=6";
var result = 0;
var match;
while ((match = expression.exec(input)) !== null) {
  result += Number(match[0]);
}
assert(result === 363, "all numeric matches in order");
assert(expression.lastIndex === 0, "failed global exec resets lastIndex");
return result;
