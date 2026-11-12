let keep={value:42}, marker={}; let p={};
p.consume=function consume(input){this.count=this.count+1;input=input.next;return input.value+this.count;};
let runner={run:function run(owner,input){let result=owner.consume(input);return result+3;}};
let owner=Object.create(p);owner.count=0;
let leaf={value:7},input={next:leaf};
for(let i=0;i<20;i++)runner.run(owner,input);
Object.defineProperty(leaf,'value',{get(){
    $262.gc();
    if(p.consume.arguments[0]!==leaf||p.consume.caller!==runner.run)throw 'reflection';
    return keep.value;
},configurable:true});
if(runner.run(owner,input)!==66||owner.count!==21)throw 'prefix';
Object.defineProperty(leaf,'value',{get(){$262.gc();throw marker;}});
try{runner.run(owner,input);throw 'missing throw';}catch(e){if(e!==marker)throw e;}
if(owner.count!==22)throw 'replay';throw keep.value;
