(function(){
  var N = 0;
  
  var r = null;
  for (var i = 0; i < N; i++) { r = [i, i + 1]; }
  if (!(N ? r[0] === N - 1 && r[1] === N : r === null)) throw new Error("per-op check: array-literal");
})();
