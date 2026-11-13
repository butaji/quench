function advance(s) { s.count = s.count - 1; }
function run(s) {
    let iterations=0;
    while(s.count>0) { advance(s); iterations++; }
    return iterations;
}
function alias(s,other) {
    let iterations=0;
    while(s.count>0) {
        other.count=other.count-1;
        if(++iterations===4)break;
    }
    return iterations;
}
function replace(s,other) {
    let iterations=0;
    while(s.count>0) { s=other; s.count--; iterations++; }
    return iterations;
}
function branchStep(s, other, choose) {
    if(choose) s=other;
    s.value=(s.value+1)|0;
}
function branchRun(s, other, choose) {
    while(s.count>0) {
        s.count=(s.count-1)|0;
        branchStep(s,other,choose);
    }
    return s.value;
}
function writeBeforeRead(s) {
    while(s.count>0) {
        s.count=(s.count-1)|0;
        s.value=7;
        s.value=(s.value+s.count)|0;
    }
    return s.value;
}
for(let i=0;i<64;i++) {
    let left={count:3,value:10},right={count:0,value:20};
    if(branchRun(left,right,false)!==13 || right.value!==20)throw 'branch original';
    left={count:3,value:10};
    if(branchRun(left,right,true)!==10 || right.value!==23)throw 'branch reassignment';
    left={count:3,value:10};
    if(branchRun(left,left,true)!==13)throw 'branch same alias';
    if(writeBeforeRead({count:3,value:99})!==7)throw 'write before read';
    if(run({count:3})!==3)throw 'integer';
    let s={count:3};
    if(alias(s,s)!==3 || s.count!==0)throw 'same alias';
    s={count:2};let other={count:9};
    if(alias(s,other)!==4 || s.count!==2 || other.count!==5)throw 'other alias';
    if(replace({count:9},{count:2})!==2)throw 'replace';
}
let s={count:2.5};
if(run(s)!==3 || s.count!==-0.5)throw 'fraction';
if(run({count:NaN})!==0 || run({count:-0})!==0)throw 'zero trip';
let calls=0;
s={count:{valueOf(){calls++;return 2;}}};
if(run(s)!==2 || calls!==2 || s.count!==0)throw 'coercion';
let gets=0,sets=0,count=3;
s={get count(){gets++;return count;},set count(v){sets++;count=v;}};
if(run(s)!==3 || gets!==7 || sets!==3 || count!==0)throw 'accessor';
let marker={},observed=0;
s={get count(){if(++observed===3)throw marker;return 2;},set count(v){}};
try{run(s);throw 'missing throw';}catch(e){if(e!==marker)throw e;}
if(observed!==3)throw 'replayed effect';
throw 42;
