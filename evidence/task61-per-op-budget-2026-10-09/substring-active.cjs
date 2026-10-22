(function(){
  var N = 200000;
  var s = "abcdefghijklmnop";
  var r = null;
  for (var i = 0; i < N; i++) { r = s.substring(3, 9); }
  if (!(r === (N ? "defghi" : null))) throw new Error("per-op check: substring");
})();
