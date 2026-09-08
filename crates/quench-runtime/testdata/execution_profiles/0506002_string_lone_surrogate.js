function inspect(value) {
  return value.repeat(2).charCodeAt(1);
}

var source = "x\ud800";
var result = inspect(source);
if (result !== 55296) {
  throw new Error("lone surrogate was not preserved: " + result);
}
return result;
