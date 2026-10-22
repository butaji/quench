Error.prepareStackTrace=(_,frames)=>frames;
function Loose(){return new Error}
function Strict(){'use strict';return Loose()}
for(const [name,fn] of [['loose',Loose],['strict',Strict]]) console.log(JSON.stringify([name,fn().stack.slice(0,3).map(s=>[s.getFunctionName(),s.getFunction()===Loose,s.getFunction()===Strict,s.getFunction()===undefined,s.getThis()===undefined])]));
