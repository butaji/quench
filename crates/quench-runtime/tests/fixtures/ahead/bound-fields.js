function calc(o) { let x=o.left, y=o.right; let sum=x+y; return sum*3; }
const o={pad:0,left:2,right:3}; let total=0;
for(let i=0;i<32;i++) total+=calc(o);
if(total!==480) throw 'wrong field arithmetic';
throw 42;
