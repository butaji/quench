(function(){
  var N = 200000;
  
  var r = null;
  for (var i = 0; i < N; i++) { r = { a: i, b: 2, c: 3 }; }
  if (!(N ? r.a === N - 1 && r.c === 3 : r === null)) throw new Error("per-op check: object-literal");
})();
