var a = []; a.push(0); for (var k=1;k<8;k++) a[k]=0;
function run(a){var r=0;for(var t=0;t<1000;t++){for(var i=0;i<1000;i++){r=a[i&7];}}return r;} var sink=run(a);
