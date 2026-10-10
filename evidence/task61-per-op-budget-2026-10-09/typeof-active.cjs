(function(){
  var N = 2000000;
  var o = {};
  var r = "";
  for (var i = 0; i < N; i++) { r = typeof o; }
  if (!(r === (N ? "object" : ""))) throw new Error("per-op check: typeof");
})();
