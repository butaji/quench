(function(){
  var N = 0;
  function P(x) { this.x = x; }
  var r = null;
  for (var i = 0; i < N; i++) { r = new P(i); }
  if (!(N ? r.x === N - 1 : r === null)) throw new Error("per-op check: construct");
})();
