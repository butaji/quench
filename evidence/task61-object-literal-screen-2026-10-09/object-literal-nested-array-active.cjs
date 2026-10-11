(function () {
  var N = 200000;
  var value = "payload";
  var result = null;
  for (var i = 0; i < N; i++) {
    result = { array: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9], string: value };
  }
  if (!(result && result.array.length === 10 && result.array[9] === 9 && result.string === value)) {
    throw new Error("nested array object literal check");
  }
})();
