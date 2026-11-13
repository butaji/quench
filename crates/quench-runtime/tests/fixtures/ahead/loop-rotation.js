function count(n){n=n|0;let result=0;for(let i=0;i<n;i=(i+1)|0)result=(result+1)|0;return result;}
function fractional(){let result=0;for(let i=0;i<2.5;i=(i+1)|0)result=(result+1)|0;return result;}
function notANumber(){let result=0;for(let i=0;i<(0/0);i=(i+1)|0)result=(result+1)|0;return result;}
function mutable(n,limit){let result=0;for(let i=0;i<n;i=(i+1)|0){result=(result+1)|0;if(i===2)n=limit;}return result;}
for(const n of [-1,0,1,2,17])if(count(n)!==(n>0?n:0))throw 'trip count';
if(fractional()!==3||notANumber()!==0)throw 'floating condition';
let probes=0,limit={valueOf(){probes++;return 3;}};
if(mutable(5,limit)!==3||probes!==1)throw 'header recovery';
throw 42;
