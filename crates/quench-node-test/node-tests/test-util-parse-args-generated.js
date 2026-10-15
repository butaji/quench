const {parseArgs}=require('node:util');
let seed=0x1a2b3c4d;
function next(n){seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed%n;}
const words=['x','--x','-x','-a','-ab','-bfoo','--beta','--beta=','--beta=z','--no-x','--','-','--bad','-c','--__proto__','--toString','\ud800','--\udfff=\ud800'];
const values=[undefined,null,false,true,0,'',{},[],['x'],[true,false],['x',2]];
const results=[];
for(let i=0;i<1200;i++){
 const config={args:Array.from({length:next(7)},()=>words[next(words.length)]),options:{x:{type:'boolean',short:'a',multiple:!!next(2)},beta:{type:'string',short:'b',multiple:!!next(2)}}};
 for(const key of ['strict','allowPositionals','tokens','allowNegative'])if(next(3))config[key]=!!next(2);
 if(next(3)===0) config.options[next(2)?'x':'beta'].default=values[next(values.length)];
 if(next(10)===0) config.options.x.short=values[next(values.length)];
 if(next(10)===0) config.options.beta.type=values[next(values.length)];
 try{results.push({result:parseArgs(config)});}catch(e){results.push({error:{name:e.name,code:e.code,message:e.message}});}
}
console.log(JSON.stringify(results));
