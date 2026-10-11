(function(){
  var N = 50000;
  var s = "foobarbaz";
  var r = null;
  for (var i = 0; i < N; i++) { r = s.replace(/o/g, "0"); }
  if (!(N ? r === "f00barbaz" : r === null)) throw new Error("per-op check: regexp-replace");
})();
