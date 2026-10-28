var a = [0,0,0,0,0,0,0,0];
function f(a) { var r=0; for (var t=0; t<4000; t++) {
    for (var i=0; i<2500; i++) { a[i&7]+=1; }
  } return a[0]; }
var result=f(a);
print(result);
