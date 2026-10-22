(function(){
  var N = 2000000;
  var o = { x: 3 };
  var r = 0;
  for (var i = 0; i < N; i++) { r = o.x; }
  if (!(r === (N ? 3 : 0))) throw new Error("per-op check: field-read");
})();
