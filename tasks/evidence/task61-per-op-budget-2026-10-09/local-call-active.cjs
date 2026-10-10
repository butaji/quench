(function(){
  var N = 1000000;
  function fn(x) { return x + 1; }
  var r = 0;
  for (var i = 0; i < N; i++) { r = fn(i); }
  if (!(r === (N ? N : 0))) throw new Error("per-op check: local-call");
})();
