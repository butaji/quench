import {execaNode} from '/workspace/quench/tests/npm-top300/node_modules/execa/index.js';
try{
 const child=execaNode('/workspace/scratch/execa-ipc-child.mjs');
 const one=child.getOneMessage();await child.sendMessage({value:'hello'});const first=await one;
 const two=child.getOneMessage();await child.sendMessage(['world',42]);const second=await two;
 const result=await child;
 console.log(JSON.stringify({complete:true,first,second,exitCode:result.exitCode,ipcOutput:result.ipcOutput}));
}catch(error){console.log(JSON.stringify({complete:true,error:error.message,cause:error.cause?.stack}));}
