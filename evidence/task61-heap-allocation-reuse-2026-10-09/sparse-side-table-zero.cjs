(function () {
  var sparse = new Array(1048576);
  var N = 0;
  var last;
  for (var i = 0; i < N; i++) last = { left: i, right: i + 1 };
  if (sparse.length !== 1048576) throw new Error("bad sparse array");
})();
