(function () {
  var N = 400000;
  var last;
  for (var i = 0; i < N; i++) last = { left: i, right: i + 1 };
  if (last.right !== N) throw new Error("bad result");
})();
