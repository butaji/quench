const vm=require('vm');
Error.prepareStackTrace=(_,frames)=>frames;
const inputs=['"for(;false;)"','#!/usr/bin/env node\nMath.max(2,3)','1; 2; 3','local','arguments[1]','new.target','"use strict"; local+=2; local','if(true){4}else{5}','try{6}finally{7}','with({local:8})local','"use strict"; 9','let x=10; x+1','return 12','/* unterminated'];
function evaluate(source,direct){let local=13;return direct?eval(source):(0,eval)(source)}
const out=[];
for(const direct of [false,true])for(const source of inputs){
 try{const value=evaluate(source,direct);out.push([direct,typeof value,String(value)])}catch(error){out.push([direct,error.name])}
}
for(const source of ['new Error().stack','1;\n\nnew Error().stack','/*leading*/\r\nnew Error().stack','"use strict"; new Error().stack','(()=>new Error().stack)()']){
 for(const direct of [false,true]){
  const frame=evaluate(source,direct)[0];out.push([direct,frame.getLineNumber(),frame.getColumnNumber(),frame.isEval()]);
 }
}
const host=vm.runInThisContext('new Error().stack',{filename:'host-eval-expr.js'})[0];
out.push(['host',host.getFileName(),host.getLineNumber(),host.getColumnNumber(),host.isEval()]);
for(const source of ['/x/g','(/x/g)',`/\ud800/`,`(/\ud800/)`]){
 const regexp=evaluate(source,true);out.push([regexp.source.charCodeAt(0),regexp.flags]);
}
console.log(JSON.stringify(out));
