function make() {
  var captured = 17;
  return function (value) { return value + captured; };
}
var fn = make();
function bench(callable) {
  var result = 0;
  for (var i = 0; i < 2000000; i++) result = callable(i);
  return result;
}
print(bench(fn));
