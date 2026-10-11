(function () {
  var N = 200000;
  var result = null;
  for (var i = 0; i < N; i++) result = { left: i, right: i + 1 };
  if (!(result && result.left === N - 1 && result.right === N)) {
    throw new Error("two-key object literal check");
  }
})();
