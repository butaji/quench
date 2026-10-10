(function () {
  function plain(a, b, c) {
    return a + b + c;
  }
  var total = 0;
  for (var i = 0; i < 250000; i++) total += plain(i, 2, 3);
  console.log(total);
})();
