function identity(value) { return value; }
function bench(fn) {
  var result = 0;
  for (var i = 0; i < 2000000; i++) result = fn(i);
  return result;
}
print(bench(identity));
