function read(values) {
  return values[1];
}

Array.prototype[1] = 41;
var values = [0, , 2];
var result = read(values);
delete Array.prototype[1];
if (result !== 41) {
  throw new Error("array hole prototype fallback mismatch");
}
return result;
