(function(){
  var N = 0;
  var a = [0,1,2,3,4,5,6,7];
  var r = 0;
  for (var i = 0; i < N; i++) { r = a[i & 7]; }
  if (!(r === (N ? 7 : 0))) throw new Error("per-op check: array-read");
})();
