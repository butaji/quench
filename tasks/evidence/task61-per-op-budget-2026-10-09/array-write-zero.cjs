(function(){
  var N = 0;
  var a = [0,0,0,0,0,0,0,0];
  var r = 0;
  for (var i = 0; i < N; i++) { a[i & 7] = i; }
  if (!(a[0] === (N ? N - 8 : 0))) throw new Error("per-op check: array-write");
})();
