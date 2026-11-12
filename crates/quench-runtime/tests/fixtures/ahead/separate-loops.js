let effects='';
function pipe(count,seed,hook){
    function step(x){x=x|0;for(let j=0;j<1;j=(j+1)|0)x=(x^(x<<3))|0;return x|0;}
    for(let warm=0;warm<3;warm++)step(warm);
    hook();
    let rounds=count,value=seed;
    for(let i=0;i<rounds;i=(i+1)|0)value=step(value);
    return value;
}
function oracle(n,x){for(let i=0;i<n;i++)x=(x^(x<<3))|0;return x;}
let hook=()=>{effects+='h';};
if(pipe(8,17,hook)!==oracle(8,17)||effects!=='h')throw 'separated loops';
let coercions=0;
let input={valueOf(){effects+='v';coercions++;return 7;}};
if(pipe(3,input,hook)!==oracle(3,7)||effects!=='hhv'||coercions!==1)throw 'coercion replay';
for(let value of [1.5,-0,2147483648,Infinity,NaN]){
    if(pipe(3,value,()=>{})!==oracle(3,value))throw 'numeric entry fallback';
}
if(pipe(3.25,7,()=>{})!==oracle(3.25,7))throw 'fractional loop bound';
let untouched={valueOf(){throw 'zero-trip coercion';}};
if(pipe(0,untouched,()=>{})!==untouched||pipe(NaN,untouched,()=>{})!==untouched)throw 'zero-trip identity';
for(let value of [1n,Symbol()]){
    try{pipe(2,value,()=>{});throw 'missing type error';}
    catch(e){if(!(e instanceof TypeError))throw e;}
}
let sentinel={};
let bad={valueOf(){throw sentinel;}};
try{pipe(2,bad,()=>{});throw 'missing coercion throw';}catch(e){if(e!==sentinel)throw e;}
try{pipe(2,3,()=>{effects+='t';throw sentinel;});throw 'missing throw';}
catch(e){if(e!==sentinel||effects!=='hhvt')throw 'opaque throw';}
function rebound(n){
    function plus(x){for(let j=0;j<1;j++)x=(x+1)|0;return x;}
    let x=0;
    for(let i=0;i<n;i++){
        if(i===2)plus=function(x){return (x+10)|0;};
        x=plus(x);
    }
    return x;
}
if(rebound(4)!==22)throw 'rebound callee';
throw 42;
