function run() {
  var result = 0;
  var captured = 1;
  var closure = function() { return captured; };
  for (var i = 0; i < 1000000; i++) result = i;
  return result + closure();
}
run();
