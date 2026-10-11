(function(){
  var N = 1000000;
  
  var r = 0;
  for (var i = 0; i < N; i++) { r = Math.floor(i / 3); }
  if (!(r === (N ? Math.floor((N - 1) / 3) : 0))) throw new Error("per-op check: math-floor");
})();
