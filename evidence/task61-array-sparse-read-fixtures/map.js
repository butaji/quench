var a = [0,0,0,0,0,0,0,0].map(function(x){return x;});
function run(a){var r=0;for(var t=0;t<1000;t++){for(var i=0;i<1000;i++){r=a[i&7];}}return r;} var sink=run(a);
