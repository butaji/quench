function add(a,b){let out=0;for(let i=0;i<1;i++){out=(a+b)|0;}return out;}
function sub(a,b){let out=0;for(let i=0;i<1;i++){out=(a-b)|0;}return out;}
function mul(a,b){let out=0;for(let i=0;i<1;i++){out=(a*b)|0;}return out;}
function observed(a,b){let sum=0,bits=0;for(let i=0;i<1;i++){sum=a+b;bits=sum|0;}return sum;}
if(add(2147483647,1)!==-2147483648 || sub(-2147483648,1)!==2147483647)throw 'overflow';
if(mul(2147483647,2147483647)!==0)throw 'rounded multiply';
if(observed(2147483647,1)!==2147483648)throw 'live intermediate';
if(add(1.5,2)!==3 || add(NaN,1)!==0 || sub(Infinity,1)!==0 ||
    add('1','2')!==12 || !Object.is(add(-0,-0),0))throw 'number fallback';
let conversions=0;
let object={valueOf(){conversions++;return 2147483647;}};
if(add(object,1)!==-2147483648 || conversions!==1)throw 'coercion replay';
throw 42;
