const pairs = [
  [(a,b)=>[(a+b)+0.5], (a,b)=>[(a+b)-0.5], (a,b)=>[(a+b)*0.5], (a,b)=>[(a+b)/0.5]],
  [(a,b)=>[(a-b)+0.5], (a,b)=>[(a-b)-0.5], (a,b)=>[(a-b)*0.5], (a,b)=>[(a-b)/0.5]],
  [(a,b)=>[(a*b)+0.5], (a,b)=>[(a*b)-0.5], (a,b)=>[(a*b)*0.5], (a,b)=>[(a*b)/0.5]],
  [(a,b)=>[(a/b)+0.5], (a,b)=>[(a/b)-0.5], (a,b)=>[(a/b)*0.5], (a,b)=>[(a/b)/0.5]],
];
function operation(op,a,b) {
  switch(op) { case 0:return a+b; case 1:return a-b; case 2:return a*b; default:return a/b; }
}
function describe(value) {
  if (Array.isArray(value)) return value.map(describe);
  if (typeof value === 'number') return Object.is(value,-0) ? '-0' : String(value);
  return typeof value + ':' + String(value);
}
const thrown = {};
let trace = [];
function input(kind, side) {
  switch(kind) {
    case 0:return 0; case 1:return -0; case 2:return 1/31; case 3:return -2.5;
    case 4:return 2147483647; case 5:return -2147483648; case 6:return Infinity;
    case 7:return -Infinity; case 8:return NaN; case 9:return '2'; case 10:return 2n;
    case 11:return null; case 12:return undefined; case 13:return Symbol(side);
    case 14:return {[Symbol.toPrimitive](hint) {trace.push(side+':'+hint); return 1/31;}};
    case 15:return {[Symbol.toPrimitive](hint) {trace.push(side+':'+hint); return '2';}};
    default:return {[Symbol.toPrimitive](hint) {trace.push(side+':'+hint); throw thrown;}};
  }
}
function observe(call, left, right) {
  trace = [];
  let result;
  try { result = ['value',describe(call(input(left,'L'),input(right,'R')))]; }
  catch(error) { result = [error === thrown ? 'identity' : error.name]; }
  return [result, trace];
}
let count = 0;
for (let first = 0; first < pairs.length; first++) {
  for (let second = 0; second < pairs[first].length; second++) {
    for (let left = 0; left < 17; left++) {
      for (let right = 0; right < 17; right++) {
        const expected = observe((a,b)=>[operation(second,operation(first,a,b),0.5)],left,right);
        const actual = observe(pairs[first][second],left,right);
        if (JSON.stringify(actual) !== JSON.stringify(expected)) throw Error('pair mismatch '+[first,second,left,right]);
        print(JSON.stringify([first,second,left,right,actual]));
        count++;
      }
    }
  }
}
for (const fn of [
  (a,b)=>{const first=a+b;return [first*0.5,first];},
  (a,b)=>[(a+b)*-0], (a,b)=>[(a+b)/0], (a,b)=>[(a+b)+3],
  (a,b)=>[(a+b)+'tail'],
]) {
  for (let left=0;left<17;left++) for(let right=0;right<17;right++) {
    print(JSON.stringify(['live-or-boundary',left,right,observe(fn,left,right)]));
    count++;
  }
}
print('cases='+count);
