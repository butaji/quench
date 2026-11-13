function pair(owner,n,reset) {let result=0; for(let i=0;i<n;i++) {if(reset)owner.value=0; const a=owner.value|0; const b=owner.value|0; result=a+b;} return result;}
function projected(owner,n,reset) {let result=0; for(let i=0;i<n;i++) {if(reset)owner.value=0; const value=owner.value; const a=value|0; const b=owner.value>>1; result=1/value+(a^b);} return result;}
function diamond(owner,other,n,write) {let result=0; for(let i=0;i<n;i++) {const first=owner.value|0; if(write)other.value=11; result=first+(owner.value|0);} return result;}
function pure(owner,n,choose,reset) {let result=0; for(let i=0;i<n;i++) {if(reset)owner.value=0; const first=owner.value|0; if(choose)result=first+1;else result=first-1; result+=owner.value|0;} return result;}
function moved(owner,other,n,choose) {let result=0; for(let i=0;i<n;i++) {const first=owner.value|0; if(choose)owner=other; result=first+(owner.value|0);} return result;}
function overwritten(owner) {let first=owner.value; first=100; return (owner.value|0)+first;}
function changed(a,b) {const first=a.value|0; b.value=9; return first+(a.value|0);}
function unrelated(owner,other,n) {let result=0; for(let i=0;i<n;i++) {const first=owner.value|0; other.other=11; result=first+(owner.value|0); owner=other;} return result;}
function liveUnrelated(owner,other,n) {let result=0; for(let i=0;i<n;i++) {const first=owner.value; other.other=11; result=(first|0)+(owner.value|0); owner=other;} return result;}
function chained(other,n) {let result=0; for(let i=0;i<n;i++) {const first=this.child; const value=this.child.value; other.other=11; if(first!==this.child)throw 'chain owner'; result=value;} return result;}
function anonymous(n) {let result=0; for(let i=0;i<n;i++) {const first=this.child.value; const second=this.child.value; result=first+second;} return result;}
function replaced(a,b) {const first=a.value|0; a=b; return first+(a.value|0);}
function callback(a,fn) {const first=a.value|0; fn(); return first+(a.value|0);}
let mask=2;
function mark(){this.value=this.value|mask;}
function marked(){const o={value:1}; o.mark=mark; return o;} function guarded(a){const first=a.value|0; const second=a.value|0; a.mark(); return first+second+(a.value|0);}
let owner={value:7};
for(let i=0;i<64;i++) {
    if(pair(owner,1,false)!==14 || overwritten(owner)!==107) throw 'plain';
    if(changed({value:7},{value:1})!==14) throw 'different owners';
    const aliased={value:7,other:1};
    if(unrelated(aliased,aliased,1,false)!==14 || aliased.other!==11) throw 'unrelated aliased field';
    if(liveUnrelated(aliased,aliased,1)!==14) throw 'live unrelated aliased field';
    const linked={child:{value:7.5},other:1};
    if(chained.call(linked,linked,1)!==7.5) throw 'chain result';
    if(anonymous.call(linked,1)!==15) throw 'anonymous chain result';
    if(replaced({value:7},{value:19})!==26) throw 'replacement';
    if(guarded(marked())!==5) throw 'guarded store';
    const joined={value:7};
    if(diamond(joined,joined,1,false)!==14 || diamond(joined,joined,1,true)!==18) throw 'join alias';
    if(diamond({value:7},{value:1},1,true)!==14) throw 'join different owner';
    if(pure(owner,1,true,false)!==15 || pure(owner,1,false,false)!==13) throw 'pure join';
    if(moved({value:7},{value:19},1,true)!==26 || moved({value:7},{value:19},1,false)!==14) throw 'join receiver replacement';
}
if(changed(owner,owner)!==16 || owner.value!==9) throw 'aliased write';
let calls=0;
const accessor={get value(){return ++calls;}};
if(pair(accessor,1,false)!==3 || calls!==2) throw 'accessor reads';
calls=0;
const coercion={value:{valueOf(){calls++;coercion.value=9;return 3;}}};
if(pair(coercion,1,false)!==12 || calls!==1) throw 'coercion mutation';
for(const [value,expected] of [[7.75,1/7.75+4],[-3.25,1/-3.25+3],
    [4294967303.5,1/4294967303.5+4],[-0,-Infinity],[NaN,NaN],[Infinity,0],[-Infinity,0]]) {
    if(!Object.is(projected({value},1,false),expected)) throw 'projection changed source';
}
calls=0;
if(projected({value:{valueOf(){return ++calls;}}},1,false)!==1/3 || calls!==3) throw 'projection skipped coercion';
owner.value=7;
if(callback(owner,()=>{owner.value=19;})!==26) throw 'callback mutation';
const setter={value:7,set other(v){this.value=v;}};
if(unrelated(setter,setter,1,false)!==18) throw 'unrelated setter effect';
setter.value=7;
if(liveUnrelated(setter,setter,1)!==18) throw 'live unrelated setter effect';
const chainSetter={child:{value:7},set other(v){this.child={value:v};}};
try {chained.call(chainSetter,chainSetter,1); throw 'missing chain effect';} catch(e) {if(e!=='chain owner')throw e;}
let childReads=0;
const childGetter={get child(){childReads++;return {value:childReads};}};
if(anonymous.call(childGetter,1)!==3 || childReads!==2) throw 'chain getter effects';
let proto={value:7}, child=Object.create(proto);
if(changed(child,proto)!==16 || child.value!==9) throw 'prototype alias';
let marker={}, prefix={count:0,value:1};
function throwing(a) {a.count=(a.count+1)|0; const first=a.value|0; return first+(a.value|0);}
for(let i=0;i<64;i++) if(throwing(prefix)!==2) throw 'warm prefix';
calls=0;
Object.defineProperty(prefix,'value',{get(){if(++calls===2)throw marker;return 4;}});
try {throwing(prefix);throw 'missing throw';} catch(e) {if(e!==marker)throw e;}
if(prefix.count!==65 || calls!==2) throw 'replayed prefix';
throw 42;
