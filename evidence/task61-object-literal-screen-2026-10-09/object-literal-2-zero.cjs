(function () {
  var N = 0;
  var result = null;
  for (var i = 0; i < N; i++) result = { left: i, right: i + 1 };
  if (result !== null) throw new Error("two-key object literal zero check");
})();
