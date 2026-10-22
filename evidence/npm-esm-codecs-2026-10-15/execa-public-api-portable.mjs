import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
import {resolve} from 'node:path';
const api=await import(pathToFileURL(resolve(process.argv[2],'index.js')).href);
const childFile=resolve(process.argv[3]);
const out=[];
const command=['-e','process.stdout.write(process.argv[1]);process.stderr.write("err")','hello world'];
for(const [name,run] of [
 ['execa',()=>api.execa(process.execPath,command)],
 ['execaSync',()=>api.execaSync(process.execPath,command)],
 ['execa options',()=>api.execa({env:{MARKER:'value'}})(process.execPath,['-e','process.stdout.write(process.env.MARKER)'])],
 ['execaNode',()=>api.execaNode(childFile,['node-child'])],
 ['script',()=>api.$({stderr:'pipe'})`${process.execPath} ${command}`],
 ['script sync',()=>api.$({stderr:'pipe'}).sync`${process.execPath} ${command}`],
]){
 try{const r=await run();out.push([name,r.stdout,r.stderr,r.exitCode,r.failed]);}
 catch(error){out.push([name,'error',error.name,error.code,error.originalMessage,error.cause?.message]);}
}
for(const input of ['a b "c d"','a "b\\"c" d',"a 'b c'",'a\\ b'])out.push(['parseCommandString',input,api.parseCommandString(input)]);
for(const name of ['ExecaError','ExecaSyncError']){const error=new api[name]('test');out.push([name,error.name,error.message,error instanceof Error,Object.getPrototypeOf(error)===api[name].prototype]);}
try{await api.execa(process.execPath,['-e','process.stderr.write("bad");process.exit(7)']);}
catch(error){out.push(['failed',error.name,error.exitCode,error.stderr,error.failed,error instanceof api.ExecaError]);}
console.log(JSON.stringify({complete:true,out}));
