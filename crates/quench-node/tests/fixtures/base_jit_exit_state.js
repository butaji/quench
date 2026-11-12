function mix(a, n, tk) { var t, k = 0, out = 0; for (var i = 0; i < n; i++) { t = (i + 7) | 0; k = (k + t) | 0; t = a[k & 63]; out = out + (t | 0); tk[i] = k; } return out + k; }
function build(n) { var a = []; for (var i = 0; i < n; i++) a.push(i * 0.75); return a; }
for (var rep = 0; rep < 300; rep++) {
  var tk = []; for (var i = 0; i < 256; i++) tk.push(0);
  var n = 200 + (rep % 17);
  mix(build(64), n, tk);
  var k = 0;
  for (var i = 0; i < n; i++) { k = (k + i + 7) | 0; if (tk[i] !== k) { console.log("BAD rep " + rep + " i " + i + " got " + tk[i] + " want " + k); break; } }
}
