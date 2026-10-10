(function(){
  var N = 0;
  function P() {} var o = new P();
  var r = false;
  for (var i = 0; i < N; i++) { r = o instanceof P; }
  if (!(r === (N > 0))) throw new Error("per-op check: instanceof");
})();
