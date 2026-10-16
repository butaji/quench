Error.prepareStackTrace=(_,frames)=>frames;
function capture(){return new Error().stack.slice(0,2).map(frame=>frame.isEval())}
function ordinary(){return capture()}
function direct(){return eval('capture()')}
const escaped=eval('(function escaped(){return capture()})');
const indirectEscaped=(0,eval)('(function indirectEscaped(){return capture()})');
const vm=require('vm');
const hostScript=vm.runInThisContext('(function hostScript(){return capture()})',{filename:'<Eval:script>'});
const retained=eval('new Error().stack');
const strictEscaped=eval("'use strict'; (function strictEscaped(){return capture()})");
for(let i=0;i<3;i++)gc();
const FunctionKinds=[Function,(async function(){}).constructor,(function*(){}).constructor,(async function*(){}).constructor];
(async()=>{
  const results=[ordinary(),direct(),(0,eval)('capture()'),escaped(),indirectEscaped(),hostScript(),eval('ordinary()'),strictEscaped(),retained.slice(0,1).map(frame=>frame.isEval())];
  for(const Kind of FunctionKinds){
    const dynamic=Kind('return capture()');
    const value=dynamic();
    results.push(typeof value?.next==='function'?(await value.next()).value:await value);
  }
  console.log(JSON.stringify(results));
})();
