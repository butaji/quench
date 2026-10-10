(function () {
  function mapped(a, b, c) {
    return arguments.length + arguments[0] + arguments[1] + arguments[2];
  }
  var total = 0;
  for (var i = 0; i < 250000; i++) total += mapped(i, 2, 3);
  console.log(total);
})();
