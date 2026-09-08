function divide(left, right) { return left / right; }

divide(4, 2);
var positive = divide(1, 0);
var negative = divide(-1, 0);
var nan = divide(0, 0);
var result = positive === Infinity && negative === -Infinity && Number.isNaN(nan);
if (!result) {
  throw new Error("number division edge mismatch");
}
return result;
