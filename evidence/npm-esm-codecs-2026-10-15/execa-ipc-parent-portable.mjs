import {pathToFileURL} from 'node:url';
import {resolve} from 'node:path';
const {execaNode}=await import(pathToFileURL(resolve(process.argv[2],'index.js')).href);
try{
 const child=execaNode(resolve(process.argv[3]),[resolve(process.argv[2])]);
 const one=child.getOneMessage();await child.sendMessage({value:'hello'});const first=await one;
 const two=child.getOneMessage();await child.sendMessage(['world',42]);const second=await two;
 const result=await child;
 console.log(JSON.stringify({complete:true,first,second,exitCode:result.exitCode,ipcOutput:result.ipcOutput}));
}catch(error){console.log(JSON.stringify({complete:true,error:error.message,cause:error.cause?.stack}));}
