(function () {
  var N = 0;
  var value = "payload";
  var result = null;
  for (var i = 0; i < N; i++) {
    result = { array: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9], string: value };
  }
  if (result !== null) throw new Error("nested array object literal zero check");
})();
