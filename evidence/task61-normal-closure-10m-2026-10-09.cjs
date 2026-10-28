(function() {
  function run() {
    var result;
    for (var outer = 0; outer < 4000; outer++) {
      for (var inner = 0; inner < 2500; inner++) {
        result = function(x) { return x; };
      }
    }
    return result;
  }
  run();
})();
