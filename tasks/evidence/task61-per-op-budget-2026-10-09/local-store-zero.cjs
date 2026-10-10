(function(){
  var N = 0;
  
  var r = 0;
  for (var i = 0; i < N; i++) { r = i; }
  if (!(r === (N ? N - 1 : 0))) throw new Error("per-op check: local-store");
})();
