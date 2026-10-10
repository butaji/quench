(function(){
  var N = 100000;
  var s = "foobarbaz";
  var r = null;
  for (var i = 0; i < N; i++) { r = /o/.exec(s); }
  if (!(N ? r[0] === "o" : r === null)) throw new Error("per-op check: regexp-exec");
})();
