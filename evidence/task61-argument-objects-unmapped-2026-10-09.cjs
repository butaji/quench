(function () {
  function unmapped(a, b, c) {
    'use strict';
    return arguments.length + arguments[0] + arguments[1] + arguments[2];
  }
  var total = 0;
  for (var i = 0; i < 250000; i++) total += unmapped(i, 2, 3);
  console.log(total);
})();
