(function(){
  var N = 0;
  
  var r = null;
  for (var i = 0; i < N; i++) { r = String.fromCharCode((i & 63) + 65); }
  if (!(N ? r.length === 1 : r === null)) throw new Error("per-op check: from-char-code");
})();
