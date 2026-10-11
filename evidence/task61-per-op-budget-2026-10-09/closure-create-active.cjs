(function(){
  var N = 100000;
  
  var r = null;
  for (var i = 0; i < N; i++) { r = function() { return i; }; }
  if (!(N ? r() === N : r === null)) throw new Error("per-op check: closure-create");
})();
