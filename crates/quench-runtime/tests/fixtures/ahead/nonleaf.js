let owner={count:0}, marker={}, keep={value:42};
owner.consume=function consume(input,callback){
    this.count=this.count+1;
    input=input.next;
    let result=callback(input);
    return result+this.count;
};
function run(owner,input,callback){let result=owner.consume(input,callback);return result+3;}
let leaf={value:7}, input={next:leaf};
function value(p){return p.value;}
for(let i=0;i<20;i++)run(owner,input,value);
function observe(p){
    $262.gc();
    if(p!==leaf||owner.consume.arguments[0]!==leaf||owner.consume.caller!==run)throw 'frame';
    return keep.value;
}
if(run(owner,input,observe)!==66||owner.count!==21)throw 'prefix';
function fail(p){$262.gc();if(p!==leaf)throw 'argument';throw marker;}
try{run(owner,input,fail);throw 'missing throw';}catch(e){if(e!==marker)throw e;}
if(owner.count!==22)throw 'replay';
throw keep.value;
