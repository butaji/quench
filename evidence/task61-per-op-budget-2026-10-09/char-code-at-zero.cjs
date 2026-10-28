(function(){
  var N = 0;
  var s = "abcdefghijklmnop";
  var r = 0;
  for (var i = 0; i < N; i++) { r = s.charCodeAt(i & 15); }
  if (!(r === (N ? 112 : 0))) throw new Error("per-op check: char-code-at");
})();
