function coupled(n){n=n|0;let i=0;for(;i<n;i=(i+1)|0){}let j=0;for(;j<i;j=(j+1)|0){}return(i+j)|0;}
function unchecked(n){n=n|0;let i=0;do{i=(i+1)|0;}while(i<n);return i;}
function reconverged(n){n=n|0;let i=0;if(!(i<n))globalThis.seen++;do{i=(i+1)|0;}while(i<n);return i;}
globalThis.seen=0;
for(const n of [-3,0,1,2,17,257]){
    if(coupled(n)!==(n>0?n*2:0))throw 'dependent bound';
    if(unchecked(n)!==(n>0?n:1))throw 'unchecked entry';
    if(reconverged(n)!==(n>0?n:1))throw 'reconverged entry';
}
if(seen!==2)throw 'entry effects';
throw 42;
